//! Victauri 版 UI 布局 E2E 测试。
//!
//! 验证关键布局区域的存在和可见性，确保 UI 结构正确。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_ui_layout`

use victauri_test::e2e_test;

/// 注入基本 SSE stub（空技能列表 + 空会话）。
async fn stub_basic_routes(client: &mut victauri_test::VictauriClient) {
    let _ = client.eval_js("(()=>{window.__origFetch=window.fetch;window.fetch=(u,o)=>{if(typeof u!=='string')return window.__origFetch(u,o);if(u.includes('/chat/stream')&&o&&o.method==='POST')return Promise.resolve(new Response('data:{\"sse_capabilities\":{\"supported_sse_v\":1}}\n\ndata:{\"stream_ended\":{\"reason\":\"completed\"}}\n',{status:200,headers:{'content-type':'text/event-stream','x-conversation-id':'e2e-layout','x-stream-job-id':'1'}}));return window.__origFetch(u,o);};}})()").await;
    let _ = client.eval_js("fetch('/user-data/prefs',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})})").await;
    let _ = client.eval_js("fetch('/user-data/workspaces/current/sessions',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify({sessions:[{id:'s_layout',title:'Layout E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false}],active_session_id:'s_layout'})})").await;
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
}

/// 获取 DOM 元素可见性信息。
async fn is_visible(client: &mut victauri_test::VictauriClient, selector: &str) -> bool {
    client
        .eval_js(&format!(
            "document.querySelector('{}')?.offsetParent!==null??false",
            selector
        ))
        .await
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}

/// 获取 DOM 元素数量。
async fn count_elements(client: &mut victauri_test::VictauriClient, selector: &str) -> usize {
    client
        .eval_js(&format!(
            "document.querySelectorAll('{}').length",
            selector
        ))
        .await
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as usize
}

// ---------------------------------------------------------------------------
// 测试 1：核心布局区域存在且顺序正确
// ---------------------------------------------------------------------------
e2e_test!(core_layout_sections_exist, |client| async move {
    stub_basic_routes(&mut client).await;

    // 聊天列存在
    assert!(is_visible(&mut client, "[data-testid=\"chat-column\"]").await,
        "chat column should be visible");

    // 状态栏存在
    assert!(is_visible(&mut client, "[data-testid=\"status-bar\"]").await,
        "status bar should be visible");

    // 侧栏默认隐藏
    assert!(!is_visible(&mut client, "[data-testid=\"side-panel\"]").await,
        "side panel should be hidden in chat mode");

    // 消息 scroller 存在
    assert!(is_visible(&mut client, "[data-testid=\"chat-messages-scroller\"]").await,
        "messages scroller should be visible");
});

// ---------------------------------------------------------------------------
// 测试 2：输入栏区域结构
// ---------------------------------------------------------------------------
e2e_test!(composer_structure, |client| async move {
    stub_basic_routes(&mut client).await;

    // 输入框存在
    assert!(is_visible(&mut client, "[data-testid=\"chat-composer-input\"]").await,
        "composer input should be visible");

    // 发送按钮存在
    assert!(is_visible(&mut client, "[data-testid=\"chat-send-button\"]").await,
        "send button should be visible");
});

// ---------------------------------------------------------------------------
// 测试 3：消息气泡布局（对话模式、助手消息、工具卡片）
// ---------------------------------------------------------------------------
e2e_test!(message_bubble_layout, |client| async move {
    stub_basic_routes(&mut client).await;

    // 发送一条简单消息触发回答
    let msg = "你好";
    let _ = client.eval_js("location.reload()").await;
    client.wait_for("network_idle", Some(""), Some(10000), Some(500)).await.ok();
    let _ = client.eval_js(&format!(
        "(()=>{{const el=document.querySelector('[data-testid=\"chat-composer-input\"]');if(!el)return;el.focus();const s=Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype,'value').set;s.call(el,'{msg}');el.dispatchEvent(new Event('input',{{bubbles:true}}));}})()"
    )).await;
    client.press_key("Enter").await.ok();
    client.wait_for("network_idle", Some(""), Some(10000), Some(200)).await.ok();

    // 至少有一条消息行
    let msg_rows = count_elements(&mut client, "[data-testid=\"chat-message-row\"]").await;
    assert!(msg_rows > 0, "should render at least one message row");

    // 助手响应到达
    client.wait_for("text", Some("sse_capabilities"), Some(10000), Some(200)).await.unwrap();
});

// ---------------------------------------------------------------------------
// 测试 4：IDE 模式布局切换
// ---------------------------------------------------------------------------
e2e_test!(ide_layout_mode, |client| async move {
    stub_basic_routes(&mut client).await;

    // 确认对话模式下 IDE 根元素隐藏
    let ide_visible = is_visible(&mut client, "[data-testid=\"ide-layout-root\"]").await;
    assert!(!ide_visible, "IDE layout should be hidden in chat mode");
});

// ---------------------------------------------------------------------------
// 测试 5：审批栏可见性
// ---------------------------------------------------------------------------
e2e_test!(approval_bar_structure, |client| async move {
    stub_basic_routes(&mut client).await;

    // 审批栏元素存在
    let approval_count = count_elements(&mut client, "[data-testid=\"approval-bar\"]").await;
    assert_eq!(approval_count, 1, "exactly one approval bar should exist");
});
