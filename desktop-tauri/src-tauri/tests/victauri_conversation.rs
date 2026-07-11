//! Victauri 版 conversation 分页加载 E2E 测试（Phase 2：需播种，无流存根）。
//!
//! 通过 `fetch('/e2e/fixtures/conversation')` 与 `/user-data/.../sessions` 播种；加载完成用 `wait_for` 断言。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_conversation`

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

const PAGINATE_CONV_ID: &str = "e2e-paginate-conv";
const PAGINATE_TOTAL: usize = 100;

/// 构造 100 条 user 消息的 JSON 数组。
fn user_messages_json(count: usize) -> String {
    (0..count)
        .map(|i| format!(r#"{{"role":"user","content":"e2e-msg-{i}"}}"#))
        .collect::<Vec<_>>()
        .join(",")
}

/// Phase 2 播种：通过 E2E fixture 创建分页对话 + 绑定会话。
async fn seed_paginated_conversation(client: &mut victauri_test::VictauriClient, session_id: &str) {
    // 设置工作区到临时目录（等价于 seedConversation 中的 isolateWorkspace）
    let _ = client
        .eval_js(
            r#"fetch('/workspace', {method:'POST', headers:{'Content-Type':'application/json'}, body: JSON.stringify({path: null})})"#,
        )
        .await;

    // 通过 E2E fixture 创建分页对话（等价于 seedConversation / seedPaginatedConversation）
    let messages = user_messages_json(PAGINATE_TOTAL);
    let _ = client
        .eval_js(&format!(
            r#"fetch('/e2e/fixtures/conversation', {{
                method: 'POST',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{conversation_id: '{PAGINATE_CONV_ID}', messages: [{messages}], replace: true}})
            }})"#
        ))
        .await;

    // 绑定会话到服务端 conversation_id（等价于 putActiveSessionWithServerConversation）
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
                    sessions: [{{id:'{session_id}',title:'E2E load older',draft:'',messages:[],updated_at:1,pinned:false,starred:false,server_conversation_id:'{PAGINATE_CONV_ID}',server_revision:1}}],
                    active_session_id: '{session_id}'
                }})
            }})"#
        ))
        .await;

    // 刷新页面触发水合
    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(15000), Some(500))
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// 测试：点击「加载更早消息」加载更早分页并隐藏按钮
// ---------------------------------------------------------------------------
e2e_test!(click_load_older_fetches_older_and_hides_control, |client| async move {
    seed_paginated_conversation(&mut client, "s_e2e_load_older").await;

    // 等待分页水合完成：尾页最新消息应可见
    client
        .wait_for(
            "text",
            Some(&format!("e2e-msg-{}", PAGINATE_TOTAL - 1)),
            Some(15000),
            Some(200),
        )
        .await
        .unwrap();

    // 确认「加载更早消息」按钮存在
    Locator::test_id("chat-load-older")
        .expect(&mut client)
        .to_be_visible()
        .await
        .unwrap();

    // 确认首条消息不可见（在更早的分页中）
    let has_msg_0: bool = client
        .eval_js("document.body.innerText.includes('e2e-msg-0')")
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
    assert!(!has_msg_0, "e2e-msg-0 should not be visible before loading older");

    // 点击「加载更早消息」
    Locator::test_id("chat-load-older")
        .click(&mut client)
        .await
        .unwrap();

    // 等待首条消息出现（表示更早分页已加载）
    client
        .wait_for("text", Some("e2e-msg-0"), Some(10000), Some(200))
        .await
        .unwrap();

    // 加载完成后 load-older 按钮应消失
    Locator::test_id("chat-load-older")
        .expect(&mut client)
        .to_be_hidden()
        .await
        .unwrap();
});
