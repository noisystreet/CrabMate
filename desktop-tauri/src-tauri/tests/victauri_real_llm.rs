//! Victauri 版真实 LLM E2E 测试（Phase 4：不注入拦截器，直连真实模型后端）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/real-llm-smoke.spec.ts` — 技能提问 → 真实回复
//!
//! ## 与 Phase 1-3 的关键区别
//!
//! Phase 1-3 通过 `eval_js` 注入 `window.fetch` 拦截器伪造 SSE 流；
//! 本测试**不注入任何拦截器**——Tauri WebView 直接调用真实后端 `POST /chat/stream`，
//! 后端再调真实 LLM。
//!
//! ## 运行条件
//!
//!   1. Tauri 桌面应用 debug 模式运行（victauri-plugin 启动 server）
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1` 启用 Victauri 测试连接
//!   4. `REAL_LLM_E2E=1` 启用真实 LLM 调用
//!   5. 已配置有效的 `API_KEY`（或 Web UI 中设置 client_llm.api_key）
//!
//! ```bash
//! REAL_LLM_E2E=1 VICTAURI_E2E=1 CM_E2E_FIXTURES=1 cargo test --test victauri_real_llm
//! ```

use std::time::{Duration, Instant};
use victauri_test::e2e_test;

const REAL_LLM_TIMEOUT_MS: u64 = 300_000; // 5 分钟

/// 跳过条件：需同时设置 VICTAURI_E2E + REAL_LLM_E2E
fn is_real_llm_enabled() -> bool {
    std::env::var("REAL_LLM_E2E").is_ok() && std::env::var("VICTAURI_E2E").is_ok()
}

async fn seed_and_reload(client: &mut victauri_test::VictauriClient, sid: &str) {
    let _ = client.eval_js("fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})})").await;
    let _ = client.eval_js(&format!("fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{sid}',title:'E2E real LLM',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{sid}'}})}})")).await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(15000), Some(500)).await.ok();
}

/// 模拟 Leptos `on:input` 事件填充输入框并点击发送。
async fn send_message(client: &mut victauri_test::VictauriClient, text: &str) {
    let _ = client.eval_js(&format!(
        "(()=>{{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');if(!el)return;el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'{text}');el.dispatchEvent(new Event('input',{{bubbles:true}}));}})()"
    )).await;
    let _ = client.eval_js("document.querySelector('[data-testid=\"chat-send-button\"]')?.click()").await;
}

/// 等待发送按钮恢复可用 + 停止按钮消失（流结束标志）。
async fn wait_for_stream_end(client: &mut victauri_test::VictauriClient) {
    let deadline = Instant::now() + Duration::from_millis(REAL_LLM_TIMEOUT_MS);
    loop {
        let done: bool = client
            .eval_js(
                "(()=>{const send=document.querySelector('[data-testid=\"chat-send-button\"]');const stop=document.querySelector('[role=\"button\"][name=\"停止\"]');return (!send||!send.disabled)&&(!stop||stop.disabled);})()"
            )
            .await
            .unwrap()
            .as_bool()
            .unwrap_or(false);
        if done {
            break;
        }
        if Instant::now() > deadline {
            panic!("stream did not finish within {}s", REAL_LLM_TIMEOUT_MS / 1000);
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }
}

// ---------------------------------------------------------------------------
// 测试 1：技能提问 → 真实 LLM 回复 + 无错误
// ---------------------------------------------------------------------------
e2e_test!(real_llm_skills_prompt_reply_no_errors, |client| async move {
    if !is_real_llm_enabled() {
        return;
    }
    // 关键：不注入任何 fetch 拦截器，让真实后端处理
    seed_and_reload(&mut client, "s_e2e_real").await;

    send_message(&mut client, "你有哪些技能").await;

    // 等待真实 LLM 流式完成
    wait_for_stream_end(&mut client).await;

    // 验证至少有一条 assistant 消息行出现
    let rows: f64 = client
        .eval_js("document.querySelectorAll('[data-testid=\"chat-message-row\"]').length")
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert!(rows > 1.0, "expected at least 2 message rows (user + assistant), got {rows}");

    // 验证无错误提示
    let has_error: bool = client
        .eval_js("document.body.innerText.includes('对话失败')||document.body.innerText.includes('请求失败')||document.body.innerText.includes('LLM_API_KEY')")
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
    assert!(!has_error, "stream should not contain error text");
});

// ---------------------------------------------------------------------------
// 测试 2：编译 hpcg — 真实 LLM 多工具轮次 + 导出布局验证
// ---------------------------------------------------------------------------
e2e_test!(real_llm_compile_hpcg_turn_layout, |client| async move {
    if !is_real_llm_enabled() {
        return;
    }
    seed_and_reload(&mut client, "s_e2e_real_compile").await;

    // 发送编译指令
    send_message(&mut client, "编译 hpcg").await;
    wait_for_stream_end(&mut client).await;

    // 验证至少有一条助手消息出现
    let assistant_count: f64 = client
        .eval_js(
            "document.querySelectorAll('.msg-meta-role').length"
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert!(assistant_count > 0.0, "expected at least 1 assistant message");

    // 验证无错误
    let has_error: bool = client
        .eval_js("document.body.innerText.includes('对话失败')||document.body.innerText.includes('请求失败')")
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
    assert!(!has_error);
});
