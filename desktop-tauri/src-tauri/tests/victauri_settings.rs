//! Victauri 版 settings-page E2E 测试。
//!
//!
//! 注意：settings-llm（secrets PUT）与 settings-mcp save 的 HTTP 拦截用例见 Phase 3（`victauri_settings2`）。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_settings`

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

/// 播种会话 + 偏好，然后刷新页面。
async fn seed_session_and_reload(
    client: &mut victauri_test::VictauriClient,
    session_id: &str,
) {
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
                    sessions: [{{id:'{session_id}',title:'E2E settings',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],
                    active_session_id: '{session_id}'
                }})
            }})"#
        ))
        .await;

    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(10000), Some(500))
        .await
        .ok();
}

/// 打开设置页面（点击 settings-open 按钮，等待设置页出现）。
async fn open_settings_page(client: &mut victauri_test::VictauriClient) {
    Locator::test_id("settings-open")
        .click(client)
        .await
        .unwrap();
    // 等待设置页可见
    client
        .wait_for(
            "selector",
            Some("[data-testid=\"settings-page\"].settings-page-visible"),
            Some(10000),
            Some(500),
        )
        .await
        .unwrap();
}

/// 点击设置导航项切换到指定 section。
async fn open_settings_section(client: &mut victauri_test::VictauriClient, section: &str) {
    Locator::test_id(&format!("settings-nav-{section}"))
        .click(client)
        .await
        .unwrap();
}

/// 关闭设置页面。
async fn close_settings_page(client: &mut victauri_test::VictauriClient) {
    Locator::test_id("settings-back")
        .click(client)
        .await
        .unwrap();
    client
        .wait_for(
            "selector_gone",
            Some("[data-testid=\"settings-page\"].settings-page-visible"),
            Some(10000),
            Some(200),
        )
        .await
        .unwrap();
}

// ---------------------------------------------------------------------------
// 测试 1：MCP nav sets URL hash（来自 settings-mcp.spec.ts）
// ---------------------------------------------------------------------------
e2e_test!(mcp_nav_sets_url_hash, |client| async move {
    seed_session_and_reload(&mut client, "s_e2e_settings_nav").await;
    open_settings_page(&mut client).await;
    open_settings_section(&mut client, "mcp").await;

    // 验证 URL hash 包含 #settings/mcp
    let hash: String = client
        .eval_js("window.location.hash || ''")
        .await
        .unwrap()
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        hash.contains("settings/mcp"),
        "expected URL hash to contain 'settings/mcp', got '{hash}'"
    );

    // 验证 MCP 设置块可见
    Locator::test_id("settings-mcp-block")
        .expect(&mut client)
        .to_be_visible()
        .await
        .unwrap();

    // 验证导航项高亮
    let nav_active: bool = client
        .eval_js(
            "document.querySelector('[data-testid=\"settings-nav-mcp\"]')?.classList.contains('active') ?? false",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(nav_active, "settings-nav-mcp should have 'active' class");
});

// ---------------------------------------------------------------------------
// 测试 2：设置页可以从主界面打开和关闭
// ---------------------------------------------------------------------------
e2e_test!(settings_page_opens_and_closes, |client| async move {
    seed_session_and_reload(&mut client, "s_e2e_settings_open").await;

    open_settings_page(&mut client).await;

    // 确认设置页可见
    Locator::test_id("settings-page")
        .expect(&mut client)
        .to_be_visible()
        .await
        .unwrap();

    close_settings_page(&mut client).await;

    // 确认设置页已关闭
    let visible: bool = client
        .eval_js(
            "document.querySelector('[data-testid=\"settings-page\"].settings-page-visible') === null",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(visible, "settings page should be closed");
});

// ---------------------------------------------------------------------------
// 测试 3：设置页 section 导航切换正确
// ---------------------------------------------------------------------------
e2e_test!(settings_section_navigation, |client| async move {
    seed_session_and_reload(&mut client, "s_e2e_settings_section").await;
    open_settings_page(&mut client).await;

    // 切换到 llm section
    open_settings_section(&mut client, "llm").await;
    let llm_hash: String = client
        .eval_js("window.location.hash || ''")
        .await
        .unwrap()
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        llm_hash.contains("settings/llm"),
        "expected hash to contain 'settings/llm', got '{llm_hash}'"
    );

    // 切换到 shortcuts section
    open_settings_section(&mut client, "shortcuts").await;
    let sc_hash: String = client
        .eval_js("window.location.hash || ''")
        .await
        .unwrap()
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        sc_hash.contains("settings/shortcuts"),
        "expected hash to contain 'settings/shortcuts', got '{sc_hash}'"
    );

    // 切换回 appearance
    open_settings_section(&mut client, "appearance").await;
    let ap_hash: String = client
        .eval_js("window.location.hash || ''")
        .await
        .unwrap()
        .as_str()
        .unwrap_or("")
        .to_string();
    assert!(
        ap_hash.contains("settings/appearance"),
        "expected hash to contain 'settings/appearance', got '{ap_hash}'"
    );

    close_settings_page(&mut client).await;
});
