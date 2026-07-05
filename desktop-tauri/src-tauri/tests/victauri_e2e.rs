//! Victauri E2E 集成测试 —— CrabMate 桌面端 MVP 自动化测试。
//!
//! 运行方式：
//!   VICTAURI_E2E=1 cargo test --test victauri_e2e
//!
//! 前置条件：先以 debug 模式启动 Tauri 桌面应用（victauri-plugin 随应用启动 server）。

use victauri_test::e2e_test;

// ---------------------------------------------------------------------------
// 冒烟测试：11 项内置检查（eval, DOM, screenshot, IPC, a11y, perf 等）
// ---------------------------------------------------------------------------
e2e_test!(smoke_test_baseline, |client| async move {
    let report = client.smoke_test().await.unwrap();
    report.assert_all_passed();
});

// ---------------------------------------------------------------------------
// 窗口状态检查
// ---------------------------------------------------------------------------
e2e_test!(window_state_check, |client| async move {
    let win = client.get_window_state(Some("main")).await.unwrap();
    // 主窗口应可见
    assert!(
        win["visible"].as_bool().unwrap_or(false),
        "main window not visible"
    );
    // 标题应为 CrabMate Desktop
    let title = win["title"].as_str().unwrap_or("");
    assert!(title.contains("CrabMate"), "unexpected window title: {title}");
});

// ---------------------------------------------------------------------------
// IPC 命令注册表：验证所有 Tauri command 已注册
// ---------------------------------------------------------------------------
e2e_test!(ipc_command_registry, |client| async move {
    let registry = client.get_registry().await.unwrap();
    let commands: Vec<&str> = registry
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c["name"].as_str())
                .collect()
        })
        .unwrap_or_default();

    let expected = &[
        "save_text_file_via_dialog",
        "pick_workspace_folder_via_dialog",
        "confirm_delete_session_via_dialog",
        "open_external_url",
        "set_main_window_decorations",
        "main_window_minimize",
        "main_window_toggle_maximize",
        "main_window_close",
    ];

    for cmd in expected {
        assert!(
            commands.contains(cmd),
            "expected Tauri command `{cmd}` not found in registry; available: {commands:?}"
        );
    }
});

// ---------------------------------------------------------------------------
// DOM 快照检查：确认 WebView 已正确加载
// ---------------------------------------------------------------------------
e2e_test!(dom_snapshot_baseline, |client| async move {
    let snapshot = client.dom_snapshot().await.unwrap();
    // 快照应不为空（已加载页面）
    let text = snapshot.to_string();
    assert!(
        !text.is_empty(),
        "DOM snapshot is empty — page may not have loaded"
    );
});

// ---------------------------------------------------------------------------
// 插件健康检查：victauri-plugin 自身运行状态
// ---------------------------------------------------------------------------
e2e_test!(plugin_health, |client| async move {
    // 服务器可达
    assert!(client.is_alive().await, "victauri server not reachable");

    // 插件信息：版本、工具数量、运行时间
    let info = client.plugin_info().await.unwrap();
    assert!(!info.version.is_empty(), "plugin version missing");
    assert!(info.tools.total > 0, "no tools registered in plugin");
    assert!(info.uptime_secs > 0.0, "plugin uptime is zero");
});
