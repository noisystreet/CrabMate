//! Victauri 版侧栏浮动工具栏 E2E（GitHub / 视图 / 设置按钮可点、DOM 结构）。
//!
//! 前置条件：
//!   1. Tauri 桌面应用 debug 模式运行
//!   2. `CM_E2E_FIXTURES=1` 启用 E2E fixture 路由
//!   3. `VICTAURI_E2E=1 cargo test --test victauri_side_toolbar`

use std::time::Duration;

use victauri_test::e2e_test;
use victauri_test::locator::Locator;

async fn seed_side_toolbar_e2e(client: &mut victauri_test::VictauriClient) {
    let _ = client
        .eval_js(
            r#"(()=>{
  window.__origFetchSideTb=window.fetch;
  window.fetch=(u,o)=>{
    if(typeof u!=='string')return window.__origFetchSideTb(u,o);
    if(u.includes('/github/repo-context')){
      return Promise.resolve(new Response(JSON.stringify({
        connected:true,
        url:'https://github.com/octocat/Hello-World',
        repo:'octocat/Hello-World'
      }),{status:200,headers:{'content-type':'application/json'}}));
    }
    if(u.includes('/github/pr/current/checks')){
      return Promise.resolve(new Response(JSON.stringify({}),{status:200,headers:{'content-type':'application/json'}}));
    }
    return window.__origFetchSideTb(u,o);
  };
})()"#,
        )
        .await;

    let _ = client
        .eval_js(
            r#"fetch('/user-data/prefs',{
  method:'PUT',
  headers:{'Content-Type':'application/json'},
  body:JSON.stringify({locale:'zh',theme:'light',side_panel_view:'hidden',side_width:280,editor_layout_mode:false})
})"#,
        )
        .await;

    let _ = client
        .eval_js(
            r#"fetch('/user-data/workspaces/current/sessions',{
  method:'PUT',
  headers:{'Content-Type':'application/json'},
  body:JSON.stringify({
    sessions:[{id:'s_e2e_side_tb',title:'Side toolbar E2E',draft:'',messages:[],updated_at:1,pinned:false,starred:false}],
    active_session_id:'s_e2e_side_tb'
  })
})"#,
        )
        .await;

    let _ = client.eval_js("location.reload()").await;
    client
        .wait_for("network_idle", Some(""), Some(10000), Some(500))
        .await
        .ok();
}

async fn wait_github_repo_btn_enabled(client: &mut victauri_test::VictauriClient) {
    for _ in 0..40 {
        let enabled: bool = client
            .eval_js(
                "(()=>{const b=document.querySelector('[data-testid=\"side-toolbar-github-repo\"]');return !!(b&&!b.disabled);})()",
            )
            .await
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if enabled {
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    panic!("side-toolbar-github-repo never became enabled");
}

// ---------------------------------------------------------------------------
// 测试 1：侧栏隐藏时浮动工具栏脱出 side-column，且父级不阻断点击
// ---------------------------------------------------------------------------
e2e_test!(
    rail_float_toolbar_dom_and_pointer_events,
    |client| async move {
        seed_side_toolbar_e2e(&mut client).await;

        let floated_outside: bool = client
        .eval_js(
            "(()=>{const col=document.querySelector('.side-column.side-column-rail-only');const tb=document.querySelector('[data-testid=\"side-shell-toolbar\"].shell-main-toolbar--rail-float');if(!col||!tb)return false;return !col.contains(tb);})()",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
        assert!(
            floated_outside,
            "floating toolbar should not be a descendant of side-column-rail-only"
        );

        let col_blocks_clicks: bool = client
        .eval_js(
            "(()=>{const col=document.querySelector('.side-column.side-column-rail-only');if(!col)return true;return getComputedStyle(col).pointerEvents==='none';})()",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(true);
        assert!(
            !col_blocks_clicks,
            "side-column-rail-only must not use pointer-events:none (blocks toolbar clicks in WebKit)"
        );
    }
);

// ---------------------------------------------------------------------------
// 测试 2：设置按钮可点击并打开设置页
// ---------------------------------------------------------------------------
e2e_test!(
    settings_toolbar_button_opens_settings_page,
    |client| async move {
        seed_side_toolbar_e2e(&mut client).await;

        Locator::test_id("settings-open")
            .click(&mut client)
            .await
            .unwrap();

        client
            .wait_for(
                "selector",
                Some("[data-testid=\"settings-page\"].settings-page-visible"),
                Some(10000),
                Some(300),
            )
            .await
            .unwrap();
    }
);

// ---------------------------------------------------------------------------
// 测试 3：视图按钮可点击并展开菜单
// ---------------------------------------------------------------------------
e2e_test!(view_toolbar_button_opens_menu, |client| async move {
    seed_side_toolbar_e2e(&mut client).await;

    Locator::test_id("side-view-trigger")
        .click(&mut client)
        .await
        .unwrap();

    let menu_visible: bool = client
        .eval_js(
            "(()=>{const el=document.querySelector('.toolbar-view-menu');return el!==null&&el.offsetParent!==null;})()",
        )
        .await
        .unwrap()
        .as_bool()
        .unwrap_or(false);
    assert!(
        menu_visible,
        "toolbar view menu should be visible after click"
    );
});

// ---------------------------------------------------------------------------
// 测试 4：GitHub 仓库按钮在 mock 连接后启用。
// Linux 使用独立 WebViewWindow；其它平台打开嵌入页并可返回。
// ---------------------------------------------------------------------------
e2e_test!(
    github_repo_button_opens_and_closes_embed_page,
    |client| async move {
        seed_side_toolbar_e2e(&mut client).await;
        wait_github_repo_btn_enabled(&mut client).await;

        Locator::test_id("side-toolbar-github-repo")
            .click(&mut client)
            .await
            .unwrap();

        #[cfg(not(target_os = "linux"))]
        {
            client
                .wait_for(
                    "selector",
                    Some("[data-testid=\"github-embed-page\"]"),
                    Some(10000),
                    Some(300),
                )
                .await
                .unwrap();

            Locator::test_id("github-embed-back")
                .click(&mut client)
                .await
                .unwrap();
        }

        client
            .wait_for(
                "selector_gone",
                Some("[data-testid=\"github-embed-page\"]"),
                Some(10000),
                Some(300),
            )
            .await
            .unwrap();
    }
);
