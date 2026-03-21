//! 集成测试：库 crate 可链接，公开 API 可用（不启动网络服务）。

#[test]
fn load_config_errors_on_missing_explicit_file() {
    assert!(crabmate::load_config(Some("/no/such/crabmate_config_9f2a.toml")).is_err());
}

#[test]
fn build_tools_returns_definitions() {
    let tools = crabmate::build_tools();
    assert!(!tools.is_empty());
    assert!(tools.iter().any(|t| t.function.name == "get_current_time"));
}
