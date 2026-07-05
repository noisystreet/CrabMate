//! Victauri E2E 集成测试 —— CrabMate 桌面端 MVP 自动化测试。

use victauri_test::e2e_test;

// ---------------------------------------------------------------------------
// 窗口状态检查：确认 main 窗口已创建
// ---------------------------------------------------------------------------
e2e_test!(window_state_check, |client| async move {
    // 等待 main 窗口出现
    for _ in 0..15 {
        let labels = client.list_windows().await.unwrap_or_default();
        if labels.to_string().contains("main") {
            eprintln!("main window found: {labels}");
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    panic!("main window did not appear after 30s");
});

// ---------------------------------------------------------------------------
// 插件健康检查：victauri-plugin 自身运行状态
// ---------------------------------------------------------------------------
e2e_test!(plugin_health, |client| async move {
    assert!(client.is_alive().await, "victauri server not reachable");
    let info = client.plugin_info().await.unwrap();
    assert!(!info.version.is_empty(), "plugin version missing");
    assert!(info.tools.total > 0, "no tools registered in plugin");
    assert!(info.uptime_secs > 0.0, "plugin uptime is zero");
});
