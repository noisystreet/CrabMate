//! Victauri 版 status-bar E2E 测试（纯 UI 子集）。
//!
//! 等价 Playwright:
//!   - `e2e/tests/status-bar.spec.ts` — status bar 存在 + agent role 菜单
//!
//! 注意：`status fetch error` 测试（依赖 `page.route()` HTTP 拦截）暂未迁移，
//! 属于 Phase 3 范围。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_status_bar`

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

/// 播种一个本地会话并刷新页面。
async fn seed_and_goto(client: &mut victauri_test::VictauriClient, session_id: &str) {
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
                    sessions: [{{id:'{session_id}',title:'E2E status',draft:'',messages:[],updated_at:1,pinned:false,starred:false}}],
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

// ---------------------------------------------------------------------------
// 测试 1：footer 状态栏在页面加载后存在
// ---------------------------------------------------------------------------
e2e_test!(footer_status_bar_present_on_load, |client| async move {
    seed_and_goto(&mut client, "s_e2e_status").await;

    Locator::test_id("status-bar")
        .expect(&mut client)
        .to_be_visible()
        .await
        .unwrap();
});

// ---------------------------------------------------------------------------
// 测试 2：点击 agent role 触发器弹出向上菜单
// ---------------------------------------------------------------------------
e2e_test!(agent_role_trigger_opens_upward_menu, |client| async move {
    seed_and_goto(&mut client, "s_e2e_role").await;

    // 确认状态栏存在
    Locator::test_id("status-bar")
        .expect(&mut client)
        .to_be_visible()
        .await
        .unwrap();

    // 点击 agent role 触发器
    Locator::test_id("status-agent-role-trigger")
        .click(&mut client)
        .await
        .unwrap();

    // 菜单应出现（通过 CSS 类定位）
    let menu_visible: bool = client
        .eval_js(
            "(() => { const el = document.querySelector('.status-agent-role-menu'); return el !== null && el.offsetParent !== null; })()",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(menu_visible, "agent role menu should be visible after clicking trigger");
});
