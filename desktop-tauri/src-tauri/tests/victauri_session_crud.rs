//! Victauri 版 session-crud E2E 测试 —— 等价于 Playwright `e2e/tests/session-crud.spec.ts`。
//!
//! 迁移映射：
//!   - `page.getByTestId(...)`       → `Locator::test_id(...)`
//!   - `.click()`                     → `.click(&mut client)`
//!   - `.toBeVisible()`              → `.expect(&mut client).to_be_visible()`
//!   - `page.keyboard.press(...)`    → `client.press_key(...)`
//!   - `page.evaluate(...)`          → `client.eval_js(...)`
//!   - `request.put(...)` (播种)     → `client.eval_js("fetch(...)")`（webview 同源）
//!   - `page.once('dialog', ...)`    → `eval_js("window.confirm = ()=>true")`
//!
//! 前置条件：
//!   1. 以 debug 模式启动 Tauri 桌面应用
//!   2. 设置 `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_session_crud`

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

/// 通过 webview 内 fetch() 播种 2 个会话，复用 Playwright 相同的 session-prefs helper 逻辑。
async fn seed_two_sessions(client: &mut victauri_test::VictauriClient) {
    // 先设置布局偏好（等价于 ensureChatLayoutPrefs）
    let _ = client
        .eval_js(
            r#"fetch('/user-data/prefs', {
                method: 'PUT',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})
            })"#,
        )
        .await;

    // 播种 2 个会话
    let _ = client
        .eval_js(
            r#"fetch('/user-data/workspaces/current/sessions', {
                method: 'PUT',
                headers: {'Content-Type': 'application/json'},
                body: JSON.stringify({
                    sessions: [
                        {id:'s_e2e_keep',title:'E2E keep',draft:'',messages:[{id:'m1',role:'user',text:'keep me'}],updated_at:2,pinned:false,starred:false},
                        {id:'s_e2e_drop',title:'E2E drop',draft:'',messages:[{id:'m2',role:'user',text:'drop me'}],updated_at:1,pinned:false,starred:false}
                    ],
                    active_session_id: 's_e2e_keep'
                })
            })"#,
        )
        .await;

    // 等待页面加载完成后再刷新以确保种子数据生效
    let _ = client.eval_js("location.reload()").await;
    // 等页面重新加载
    client
        .wait_for("network_idle", Some(""), Some(10000), Some(500))
        .await
        .ok();
}

/// 右键点击 nav-rail 打开上下文菜单，点击「管理会话」进入会话管理模态框。
async fn open_session_list_modal(client: &mut victauri_test::VictauriClient) {
    let _ = client
        .eval_js(
            r#"(() => {
                const rail = document.querySelector('.nav-rail-scroll');
                if (rail) {
                    rail.dispatchEvent(new MouseEvent('contextmenu', {bubbles: true, button: 2}));
                }
            })()"#,
        )
        .await;
    // 点击上下文菜单中的「管理会话」
    Locator::text("管理会话").click(client).await.ok();
    // 等待模态框出现
    Locator::test_id("session-list-modal")
        .expect(client)
        .to_be_visible()
        .await
        .ok();
}

// ---------------------------------------------------------------------------
// 测试 1：新建会话 → rail 出现第三条 nav-session
// ---------------------------------------------------------------------------
e2e_test!(new_chat_creates_session_in_rail, |client| async move {
    seed_two_sessions(&mut client).await;

    // 确认初始有 2 条会话
    let count: f64 = client
        .eval_js(
            "document.querySelectorAll('[data-testid^=\"nav-session-\"]').length",
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert_eq!(count as usize, 2, "expected 2 sessions, got {count}");

    // 点击「新建对话」
    Locator::test_id("nav-new-chat").click(&mut client).await.unwrap();

    // 等待 rail 中出现第三条
    client
        .wait_for("selector", Some("[data-testid^=\"nav-session-\"]:nth-child(3)"), Some(5000), Some(200))
        .await
        .unwrap();

    let count_after: f64 = client
        .eval_js(
            "document.querySelectorAll('[data-testid^=\"nav-session-\"]').length",
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert_eq!(count_after as usize, 3, "expected 3 sessions after new chat");
});

// ---------------------------------------------------------------------------
// 测试 2：在管理模态框中固定会话 → rail 条目出现 is-pinned CSS 类
// ---------------------------------------------------------------------------
e2e_test!(pin_session_shows_badge_in_rail, |client| async move {
    seed_two_sessions(&mut client).await;
    open_session_list_modal(&mut client).await;

    // 点击固定按钮
    Locator::test_id("session-modal-pin-s_e2e_drop")
        .click(&mut client)
        .await
        .unwrap();

    // 关闭模态框
    client.press_key("Escape").await.unwrap();
    Locator::test_id("session-list-modal")
        .expect(&mut client)
        .to_be_hidden()
        .await
        .unwrap();

    // 验证 rail 条目有 is-pinned 类
    let has_pinned: bool = client
        .eval_js(
            "document.querySelector('[data-testid=\"nav-session-s_e2e_drop\"]')?.classList.contains('is-pinned') ?? false",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(has_pinned, "expected nav-session-s_e2e_drop to have class 'is-pinned'");
});

// ---------------------------------------------------------------------------
// 测试 3：确认删除后会话从模态框列表和 rail 中移除
// ---------------------------------------------------------------------------
e2e_test!(delete_session_after_confirm_removes_it, |client| async move {
    seed_two_sessions(&mut client).await;
    open_session_list_modal(&mut client).await;

    // 覆盖 window.confirm 使其始终返回 true
    let _ = client.eval_js("window.confirm = () => true").await;

    // 点击删除按钮
    Locator::test_id("session-modal-delete-s_e2e_drop")
        .click(&mut client)
        .await
        .unwrap();

    // 等待该会话从模态框列表中消失
    client
        .wait_for(
            "selector_gone",
            Some("[data-testid=\"session-modal-open-s_e2e_drop\"]"),
            Some(5000),
            Some(200),
        )
        .await
        .unwrap();

    // 确认 rail 中也已移除
    let rail_gone: bool = client
        .eval_js(
            "document.querySelector('[data-testid=\"nav-session-s_e2e_drop\"]') === null",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(rail_gone, "nav-session-s_e2e_drop should be removed from rail after delete");
});

// ---------------------------------------------------------------------------
// 测试 4：模态框关闭后 rail 保持初始状态（Escape 关闭不产生副作用）
// ---------------------------------------------------------------------------
e2e_test!(escape_closes_modal_without_side_effects, |client| async move {
    seed_two_sessions(&mut client).await;
    open_session_list_modal(&mut client).await;

    // 按 Escape 关闭
    client.press_key("Escape").await.unwrap();

    // 模态框不可见
    Locator::test_id("session-list-modal")
        .expect(&mut client)
        .to_be_hidden()
        .await
        .unwrap();

    // rail 中仍应有 2 条会话
    let count: f64 = client
        .eval_js(
            "document.querySelectorAll('[data-testid^=\"nav-session-\"]').length",
        )
        .await
        .unwrap()
        .as_f64()
        .unwrap_or(0.0);
    assert_eq!(count as usize, 2, "expected 2 sessions after closing modal");
});

// ---------------------------------------------------------------------------
// 测试 5（session-list.spec.ts）：模态框切换活动会话
// ---------------------------------------------------------------------------
e2e_test!(manage_sessions_modal_switches_active_session, |client| async move {
    seed_two_sessions(&mut client).await;

    // 确认初始活动会话内容可见
    client
        .expect_text("keep me")
        .await
        .unwrap();

    open_session_list_modal(&mut client).await;

    // 点击切换到 session B
    Locator::test_id("session-modal-open-s_e2e_drop")
        .click(&mut client)
        .await
        .unwrap();

    // 模态框应关闭
    Locator::test_id("session-list-modal")
        .expect(&mut client)
        .to_be_hidden()
        .await
        .unwrap();

    // 确认切换后显示 session B 的内容
    client
        .expect_text_with_timeout("drop me", 15000)
        .await
        .unwrap();

    // 确认旧内容不再可见
    client
        .expect_no_text("keep me")
        .await
        .unwrap();
});
