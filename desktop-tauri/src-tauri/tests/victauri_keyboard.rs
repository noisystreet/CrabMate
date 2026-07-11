//! Victauri 版 keyboard-shortcuts E2E 测试（Phase 2：需播种，无流存根）。
//!
//! Phase 2 技术要点：
//!   1. API 播种：`eval_js("fetch('/user-data/...', {method:'PUT', ...})")`
//!   2. 滚动断言：`eval_js` 轮询 `scrollTop`
//!
//! 注意：Enter 发送 + SSE 存根见 Phase 3（`victauri_sse_stub` 等）。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_keyboard`

use std::time::{Duration, Instant};
use victauri_test::e2e_test;

/// Phase 2 播种模式：通过 webview `fetch()` 调用后端 PUT API 写入 40 条消息。
async fn seed_sessions_with_messages(
    client: &mut victauri_test::VictauriClient,
    session_id: &str,
    count: usize,
    prefix: &str,
) {
    // 构造消息数组 JSON
    let messages_json: String = (0..count)
        .map(|i| format!(r#"{{"id":"m_{prefix}_{i}","role":"user","text":"{prefix}-line-{i}"}}"#))
        .collect::<Vec<_>>()
        .join(",");

    let _ = client
        .eval_js(r#"fetch('/user-data/prefs', {
                method: 'PUT',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})
            })"#)
        .await;

    let _ = client
        .eval_js(&format!(
            r#"fetch('/user-data/workspaces/current/sessions', {{
                method: 'PUT',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{
                    sessions: [{{id:'{session_id}',title:'E2E scroll',draft:'',messages:[{messages_json}],updated_at:1,pinned:false,starred:false}}],
                    active_session_id: '{session_id}'
                }})
            }})"#
        ))
        .await;

    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(15000), Some(500))
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// 测试 1：End 键滚底（来自 keyboard-shortcuts.spec.ts）
// ---------------------------------------------------------------------------
e2e_test!(end_key_scrolls_messages_toward_bottom, |client| async move {
    seed_sessions_with_messages(&mut client, "s_e2e_keys", 40, "e2e-scroll").await;

    // 确认页面已加载
    client
        .wait_for("text", Some("e2e-scroll-line-0"), Some(15000), Some(200))
        .await
        .unwrap();

    // 聚焦输入框并按 Home 滚到顶
    let _ = client
        .eval_js(
            "document.querySelector('[data-testid=\"chat-composer-input\"]')?.focus()",
        )
        .await;

    client.press_key("Home").await.unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let top: f64 = client
            .eval_js(
                "(document.querySelector('[data-testid=\"chat-messages-scroller\"]')?.scrollTop) ?? -1",
            )
            .await
            .unwrap()
            .as_f64()
            .unwrap_or(-1.0);
        if top == 0.0 {
            break;
        }
        if Instant::now() > deadline {
            panic!("Home key did not scroll to top within 10s, scrollTop={top}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // 按 End 滚到底
    client.press_key("End").await.unwrap();

    // 轮询等待滚到底
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let result: bool = client
            .eval_js(
                r#"(() => {
                    const el = document.querySelector('[data-testid="chat-messages-scroller"]');
                    if (!el) return false;
                    const max = el.scrollHeight - el.clientHeight;
                    return max > 0 && el.scrollTop >= max - 4;
                })()"#,
            )
            .await
            .unwrap()
            .as_bool()
            .unwrap_or(false);
        if result {
            break;
        }
        if Instant::now() > deadline {
            panic!("End key did not scroll to bottom within 10s");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
});

// ---------------------------------------------------------------------------
// 测试 3（Phase 3）：Enter 发送消息出现助手回复（fetch 拦截器存根 SSE 流）
// ---------------------------------------------------------------------------
e2e_test!(enter_sends_message_with_stream_stub, |client| async move {
    // Phase 3: 注入 SSE 流存根
    let sse = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: Hello from E2E stub.\n\n",
        "id: 4\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n",
    );
    let _ = client.eval_js(&format!(
        "(()=>{{const body=`{sse}`;window.__kOrigFetch=window.fetch;\
         window.fetch=(u,o)=>{{if(typeof u==='string'&&u.includes('/chat/stream')&&o&&o.method==='POST')\
         return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream'}}}}));\
         return window.__kOrigFetch(u,o);}};}})()"
    )).await;

    seed_sessions_with_messages(&mut client, "s_e2e_keys_enter", 2, "enter-test").await;

    let _ = client.eval_js(
        "(()=>{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');if(!el)return;el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'e2e enter send');el.dispatchEvent(new Event('input',{bubbles:true}));})()"
    ).await;

    client.press_key("Enter").await.unwrap();

    client.wait_for("text", Some("Hello from E2E stub"), Some(15000), Some(200)).await.unwrap();
});

// ---------------------------------------------------------------------------
// 测试 2：Home 键滚顶
// ---------------------------------------------------------------------------
e2e_test!(home_key_scrolls_to_top, |client| async move {
    seed_sessions_with_messages(&mut client, "s_e2e_keys_home", 40, "e2e-home").await;

    client
        .wait_for("text", Some("e2e-home-line-0"), Some(15000), Some(200))
        .await
        .unwrap();

    // 先滚到底
    let _ = client.eval_js(
        "(() => { const el = document.querySelector('[data-testid=\"chat-messages-scroller\"]'); if (el) el.scrollTop = el.scrollHeight; })()",
    ).await;

    // 确保不在顶部
    let top: f64 = client
        .eval_js(
            "(document.querySelector('[data-testid=\"chat-messages-scroller\"]')?.scrollTop) ?? 0",
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert!(top > 0.0, "expected scrolled away from top, got scrollTop={top}");

    // 按 Home
    client.press_key("Home").await.unwrap();

    // 轮询等待滚到顶
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let top: f64 = client
            .eval_js(
                "(document.querySelector('[data-testid=\"chat-messages-scroller\"]')?.scrollTop) ?? -1",
            )
            .await
            .unwrap()
            .as_f64()
            .unwrap_or(-1.0);
        if top == 0.0 {
            break;
        }
        if Instant::now() > deadline {
            panic!("Home key did not scroll to top, scrollTop={top}");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
});
