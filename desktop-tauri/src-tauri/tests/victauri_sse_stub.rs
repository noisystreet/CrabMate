//! Victauri 版 Phase 3 E2E 测试演示：`page.route()` HTTP 拦截 → `eval_js` 注入 fetch 拦截器。
//!
//! 等价 Playwright:
//!   - `e2e/tests/smoke.spec.ts`               — 发送消息 + 工具卡渲染
//!   - `e2e/tests/helpers/fix-chat-stream.ts`   — SSE 流存根构建器
//!
//! ## Phase 3 核心技术
//!
//! Playwright 的 `page.route()` 在浏览器网络层拦截 HTTP 请求。Victauri 无法做同层拦截，
//! 但可以通过 `eval_js` 在 webview 内覆盖 `window.fetch`，实现等价效果：
//!
//! ```javascript
//! // Playwright
//! await page.route('**/chat/stream', route => route.fulfill({status:200, body:sseBody}));
//!
//! // Victauri
//! client.eval_js("window.__originalFetch = window.fetch;
//!   window.fetch = (url, opts) => {
//!     if (url.includes('/chat/stream') && opts.method==='POST')
//!       return Promise.resolve(new Response(sseBody, {status:200,headers:{'content-type':'text/event-stream'}}));
//!     return window.__originalFetch(url, opts);
//!   }");
//! ```
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_sse_stub`

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

/// Phase 3 核心：注入 fetch 拦截器以存根 SSE 流。
/// 等价于 Playwright 的 `installChatStreamStub(page)`。
async fn install_chat_stream_stub(client: &mut victauri_test::VictauriClient) {
    // 构造默认 SSE 流：助手 delta + diagnostic_summary 工具卡 + stream_ended
    let sse_body = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: Hello from E2E stub via Victauri.\n\n",
        "id: 4\ndata: {\"tool_result\":{\"name\":\"diagnostic_summary\",\"result_version\":1,\"summary\":\"ok\",\"output\":\"stub\",\"ok\":true}}\n\n",
        "id: 5\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n",
    );

    let _ = client
        .eval_js(&format!(
            "(()=>{{const body=`{sse_body}`;\
             window.__originalFetch=window.fetch;\
             window.fetch=(url,opts)=>{{if(typeof url==='string'&&url.includes('/chat/stream')&&opts&&opts.method==='POST')\
             return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream; charset=utf-8','x-conversation-id':'e2e-conv','x-stream-job-id':'1'}}}}));\
             return window.__originalFetch(url,opts);}};}})()"
        ))
        .await;
}

/// 播种默认会话并重新加载。
async fn seed_and_goto(client: &mut victauri_test::VictauriClient, session_id: &str) {
    let _ = client
        .eval_js(&format!(
            "fetch('/user-data/prefs',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false}})}})"
        ))
        .await;

    let _ = client
        .eval_js(&format!(
            "fetch('/user-data/workspaces/current/sessions',{{method:'PUT',headers:{{'Content-Type':'application/json'}},body:JSON.stringify({{sessions:[{{id:'{session_id}',title:'E2E smoke',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],active_session_id:'{session_id}'}})}})"
        ))
        .await;

    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(10000), Some(500))
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// 测试 1：发送消息后出现助手回复和工具卡（Phase 3 流存根演示）
// ---------------------------------------------------------------------------
e2e_test!(send_message_shows_assistant_reply_and_tool_card, |client| async move {
    // Phase 3: 先注入 fetch 拦截器 → 再加载页面
    install_chat_stream_stub(&mut client).await;
    seed_and_goto(&mut client, "s_e2e_smoke").await;

    // 等待主页面加载
    let _ = client
        .wait_for("text", Some("CrabMate"), Some(10000), Some(200))
        .await;

    // 在输入框中填入文本
    let _ = client
        .eval_js(
            "(()=>{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');\
             if(!el)return;el.focus();\
             const nativeInputValueSetter=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;\
             nativeInputValueSetter.call(el,'e2e ping');\
             el.dispatchEvent(new Event('input',{{bubbles:true}}));})()"
        )
        .await;

    // 按 Enter 发送（等价于 sendStubMessage）
    client.press_key("Enter").await.unwrap();

    // 等待 SSE 流存根的助手回复出现
    client
        .wait_for(
            "text",
            Some("Hello from E2E stub via Victauri"),
            Some(15000),
            Some(200),
        )
        .await
        .unwrap();

    // 验证工具卡出现
    Locator::test_id("chat-tool-card")
        .expect(&mut client)
        .to_be_visible()
        .await
        .unwrap();
});

// ---------------------------------------------------------------------------
// 测试 2：流错误存根 → 状态栏显示失败
// ---------------------------------------------------------------------------
e2e_test!(stream_error_shows_failure_in_status_bar, |client| async move {
    // Phase 3: 注入错误流存根
    let sse_body = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: {\"error\":\"e2e intentional failure\",\"code\":\"E2E_STREAM_FAIL\"}\n\n",
        "id: 4\ndata: {\"stream_ended\":{\"reason\":\"error\"}}\n\n",
    );

    let _ = client
        .eval_js(&format!(
            "(()=>{{const body=`{sse_body}`;\
             window.__originalFetch=window.fetch;\
             window.fetch=(url,opts)=>{{if(typeof url==='string'&&url.includes('/chat/stream')&&opts&&opts.method==='POST')\
             return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream'}}}}));\
             return window.__originalFetch(url,opts);}};}})()"
        ))
        .await;

    seed_and_goto(&mut client, "s_e2e_err").await;

    // 发送消息
    let _ = client
        .eval_js(
            "(()=>{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');\
             if(!el)return;el.focus();\
             const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;\
             s.call(el,'e2e error test');el.dispatchEvent(new Event('input',{{bubbles:true}}));})()"
        )
        .await;

    client.press_key("Enter").await.unwrap();

    // 等待状态栏显示 fetch-error 样式
    client
        .wait_for(
            "selector",
            Some("[data-testid=\"status-bar\"].status-bar-fetch-error"),
            Some(15000),
            Some(200),
        )
        .await
        .unwrap();
});
