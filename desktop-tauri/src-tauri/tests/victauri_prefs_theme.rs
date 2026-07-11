//! Victauri 版 prefs-theme + prefs-side-panel E2E 测试。
//!
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行（victauri-plugin 启动 server）
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_prefs_theme`

use victauri_test::e2e_test;

/// 通过 webview 内 fetch() 设置用户偏好并重新加载页面。
async fn seed_and_reload(
    client: &mut victauri_test::VictauriClient,
    session_id: &str,
    theme: &str,
    side_panel: &str,
    side_width: u32,
) {
    // 播种偏好
    let _ = client
        .eval_js(&format!(
            r#"fetch('/user-data/prefs', {{
                method: 'PUT',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{locale:'zh',theme:'{theme}',side_panel_view:'{side_panel}',side_width:{side_width},editor_layout_mode:false}})
            }})"#
        ))
        .await;

    // 播种会话
    let _ = client
        .eval_js(&format!(
            r#"fetch('/user-data/workspaces/current/sessions', {{
                method: 'PUT',
                headers: {{'Content-Type': 'application/json'}},
                body: JSON.stringify({{
                    sessions: [{{id:'{session_id}',title:'E2E theme',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],
                    active_session_id: '{session_id}'
                }})
            }})"#
        ))
        .await;

    // 刷新页面使预设置生效
    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(10000), Some(500))
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// prefs-theme.spec.ts: dark theme from user-data/prefs applies data-theme
// ---------------------------------------------------------------------------
e2e_test!(dark_theme_applies_data_theme_on_load, |client| async move {
    seed_and_reload(&mut client, "s_e2e_theme", "dark", "hidden", 280).await;

    // 等待页面重新加载
    client
        .wait_for("text", Some("CrabMate"), Some(15000), Some(200))
        .await
        .unwrap();

    // 验证 html 元素 data-theme 属性
    let theme: String = client
        .eval_js("document.documentElement.getAttribute('data-theme') || ''")
        .await
        .unwrap()
        .as_str()
        .unwrap_or("")
        .to_string();
    assert_eq!(theme, "dark", "expected data-theme='dark', got '{theme}'");
});

// ---------------------------------------------------------------------------
// prefs-side-panel.spec.ts: workspace side panel opens on load
// ---------------------------------------------------------------------------
e2e_test!(workspace_panel_opens_on_load_when_prefs_say_workspace, |client| async move {
    seed_and_reload(&mut client, "s_e2e_side", "light", "workspace", 320).await;

    // 等待主页面出现
    client
        .wait_for("text", Some("CrabMate"), Some(15000), Some(200))
        .await
        .unwrap();

    // 验证工作区面板可见
    let panel_visible: bool = client
        .eval_js(
            "(() => { const el = document.querySelector('[data-testid=\"workspace-panel\"]'); return el !== null && el.offsetParent !== null; })()",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(panel_visible, "workspace-panel should be visible when prefs say 'workspace'");
});
