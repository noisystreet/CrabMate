//! Victauri 版发送消息后自动滚底 E2E 测试。
//!
//! 验证：用户发送消息后，聊天视图自动滚动到最新消息处。
//!
//! 等价 Playwright:
//!   - 无直接对应（原 Playwright suite 未覆盖此路径）
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_scroll_send`

use std::time::{Duration, Instant};
use victauri_test::e2e_test;

/// 播种含大量消息的会话（使聊天可滚动）。
async fn seed_scrollable_session(
    client: &mut victauri_test::VictauriClient,
    session_id: &str,
    count: usize,
) {
    let messages_json: String = (0..count)
        .map(|i| format!(r#"{{"id":"m_{i}","role":"user","text":"scroll-test-line-{i}"}}"#))
        .collect::<Vec<_>>()
        .join(",");

    let _ = client
        .eval_js(
            r#"fetch('/user-data/prefs', {
                method: 'PUT',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})
            })"#,
        )
        .await;

    let _ = client
        .eval_js(&format!(
            r#"fetch('/user-data/workspaces/current/sessions', {{
                method: 'PUT',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{
                    sessions: [{{id:'{session_id}',title:'E2E scroll-send',draft:'',messages:[{messages_json}],updated_at:1,pinned:false,starred:false}}],
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

/// 注入 SSE 流存根，使发送消息后能收到助手回复。
async fn inject_stream_stub(client: &mut victauri_test::VictauriClient, sse_body: &str) {
    let _ = client
        .eval_js(&format!(
            "(()=>{{const body=`{sse_body}`;\
             window.__origFetch=window.fetch;\
             window.fetch=(url,opts)=>{{if(typeof url==='string'&&url.includes('/chat/stream')&&opts&&opts.method==='POST')\
             return Promise.resolve(new Response(body,{{status:200,headers:{{'content-type':'text/event-stream'}}}}));\
             return window.__origFetch(url,opts);}};}})()"
        ))
        .await;
}

/// 检查滚动条是否在底部（容差 4px）。
fn is_at_bottom(result: &serde_json::Value) -> bool {
    result.as_bool().unwrap_or(false)
}

/// 轮询等待滚动条到达底部。
async fn poll_scroll_at_bottom(
    client: &mut victauri_test::VictauriClient,
    timeout_secs: u64,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let at_bottom = client
            .eval_js(
                r#"(() => {
                    const el = document.querySelector('[data-testid="chat-messages-scroller"]');
                    if (!el) return false;
                    const max = el.scrollHeight - el.clientHeight;
                    return max > 0 && el.scrollTop >= max - 4;
                })()"#,
            )
            .await
            .map_err(|e| e.to_string())
            .and_then(|v| {
                if is_at_bottom(&v) {
                    Ok(())
                } else {
                    Err("not at bottom".to_string())
                }
            });
        if at_bottom.is_ok() {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("scroll did not reach bottom within {timeout_secs}s"));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

/// 轮询等待滚动条离开底部。
async fn poll_scroll_away_from_bottom(
    client: &mut victauri_test::VictauriClient,
    timeout_secs: u64,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        let away = client
            .eval_js(
                r#"(() => {
                    const el = document.querySelector('[data-testid="chat-messages-scroller"]');
                    if (!el) return false;
                    const max = el.scrollHeight - el.clientHeight;
                    return max > 0 && el.scrollTop < max - 4;
                })()"#,
            )
            .await
            .map_err(|e| e.to_string())
            .and_then(|v| {
                if v.as_bool().unwrap_or(false) {
                    Ok(())
                } else {
                    Err("still at bottom".to_string())
                }
            });
        if away.is_ok() {
            return Ok(());
        }
        if Instant::now() > deadline {
            return Err(format!("scroll did not leave bottom within {timeout_secs}s"));
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

// ---------------------------------------------------------------------------
// 测试 1：已有消息的会话中发送新消息 → 自动滚底
// ---------------------------------------------------------------------------
e2e_test!(send_message_scrolls_to_bottom_in_existing_chat, |client| async move {
    // 注入 SSE 流存根
    let sse = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: Hello from E2E scroll test.\n\n",
        "id: 4\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n",
    );
    inject_stream_stub(&mut client, sse).await;

    // 播种 40 条消息的会话
    seed_scrollable_session(&mut client, "s_e2e_scroll_send", 40).await;

    // 确认页面加载
    client
        .wait_for("text", Some("scroll-test-line-0"), Some(15000), Some(200))
        .await
        .unwrap();

    // 先滚到顶部，确保不在底部
    let _ = client
        .eval_js(
            "document.querySelector('[data-testid=\"chat-messages-scroller\"]')?.scrollTo(0, 0)",
        )
        .await;

    poll_scroll_away_from_bottom(&mut client, 10)
        .await
        .expect("should be away from bottom after scrolling to top");

    // 输入消息并发送
    let _ = client
        .eval_js(
            "(()=>{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');\
             if(!el)return;el.focus();\
             const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;\
             s.call(el,'e2e scroll send test');\
             el.dispatchEvent(new Event('input',{bubbles:true}));})()",
        )
        .await;

    client.press_key("Enter").await.unwrap();

    // 等待助手回复出现（确认流已处理）
    client
        .wait_for(
            "text",
            Some("Hello from E2E scroll test"),
            Some(15000),
            Some(200),
        )
        .await
        .unwrap();

    // 验证滚动条已到达底部
    poll_scroll_at_bottom(&mut client, 10)
        .await
        .expect("should scroll to bottom after sending message");
});

// ---------------------------------------------------------------------------
// 测试 2：空会话中发送首条消息 → 自动滚底
// ---------------------------------------------------------------------------
e2e_test!(send_first_message_scrolls_to_bottom, |client| async move {
    let sse = concat!(
        "id: 1\ndata: {\"sse_capabilities\":{\"supported_sse_v\":1}}\n\n",
        "id: 2\ndata: {\"v\":1}\n\n",
        "id: 3\ndata: First message reply.\n\n",
        "id: 4\ndata: {\"stream_ended\":{\"reason\":\"completed\"}}\n\n",
    );
    inject_stream_stub(&mut client, sse).await;

    // 播种空会话
    let _ = client
        .eval_js(
            "fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})})",
        )
        .await;
    let _ = client
        .eval_js(
            "fetch('/user-data/workspaces/current/sessions',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({sessions:[{id:'s_e2e_empty',title:'E2E empty',draft:'',messages:[],updated_at:1,pinned:false,starred:false}],active_session_id:'s_e2e_empty'})})",
        )
        .await;
    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(10000), Some(500))
        .await
        .ok();

    // 输入消息并发送
    let _ = client
        .eval_js(
            "(()=>{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');\
             if(!el)return;el.focus();\
             const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;\
             s.call(el,'first message');\
             el.dispatchEvent(new Event('input',{bubbles:true}));})()",
        )
        .await;

    client.press_key("Enter").await.unwrap();

    // 等待助手回复出现
    client
        .wait_for("text", Some("First message reply"), Some(15000), Some(200))
        .await
        .unwrap();

    // 验证滚动条已到达底部
    poll_scroll_at_bottom(&mut client, 10)
        .await
        .expect("should scroll to bottom after sending first message in empty session");
});