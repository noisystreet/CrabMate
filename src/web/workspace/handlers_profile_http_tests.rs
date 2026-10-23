//! `GET /workspace/profile` 的轻量 HTTP 冒烟：总开关（`project_profile_inject_enabled`）
//! 与字符上限（`project_profile_inject_max_chars`）。
//!
//! 背景：项目画像已不进首轮注入，仅由该路由（Web 侧栏）与 `repo_overview_sweep` 按需生成；
//! 本文件锁定「开关关闭 / 上限为 0 → 空正文且无 error」这条分支。

use crate::test_serve::start_test_serve_with_config;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

/// 最小可识别为 Rust 单包的临时工作区（`Cargo.toml` + `src/lib.rs`）。
fn temp_cargo_workspace() -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir_all(root.path().join("src")).expect("mkdir src");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"profile_demo\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .expect("write Cargo.toml");
    std::fs::write(root.path().join("src/lib.rs"), "pub fn f() {}\n").expect("write lib.rs");
    root
}

fn cfg_with_profile(enabled: bool, max_chars: usize) -> crate::config::AgentConfig {
    let mut cfg = crate::config::load_config(None).expect("embed default");
    cfg.context_bootstrap_inject
        .project_profile_inject_enabled = enabled;
    cfg.context_bootstrap_inject
        .project_profile_inject_max_chars = max_chars;
    cfg
}

/// 设置工作区后请求 `/workspace/profile`，返回解析后的 JSON 响应体。
async fn fetch_profile(
    client: &reqwest::Client,
    base: &str,
    workspace: &std::path::Path,
) -> serde_json::Value {
    let set = client
        .post(format!("{base}/workspace"))
        .json(&serde_json::json!({ "path": workspace }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(set.status().is_success(), "set workspace");
    let resp = client
        .get(format!("{base}/workspace/profile"))
        .send()
        .await
        .expect("GET /workspace/profile");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    resp.json().await.expect("profile json body")
}

#[tokio::test]
async fn workspace_profile_returns_body_when_enabled() {
    let ws = temp_cargo_workspace();
    let handle = start_test_serve_with_config(cfg_with_profile(true, 6000), None).await;
    let client = loopback_http_client();

    let body = fetch_profile(&client, &handle.base_url, ws.path()).await;
    assert!(body["error"].is_null(), "unexpected error: {body}");
    let md = body["markdown"].as_str().unwrap_or_default();
    assert!(md.contains("项目画像"), "profile body: {md}");
    assert!(md.contains("profile_demo"), "profile body: {md}");
}

#[tokio::test]
async fn workspace_profile_empty_when_master_switch_off() {
    let ws = temp_cargo_workspace();
    let handle = start_test_serve_with_config(cfg_with_profile(false, 6000), None).await;
    let client = loopback_http_client();

    let body = fetch_profile(&client, &handle.base_url, ws.path()).await;
    assert_eq!(body["markdown"].as_str(), Some(""));
    assert!(body["error"].is_null(), "unexpected error: {body}");
}

#[tokio::test]
async fn workspace_profile_empty_when_max_chars_zero() {
    let ws = temp_cargo_workspace();
    let handle = start_test_serve_with_config(cfg_with_profile(true, 0), None).await;
    let client = loopback_http_client();

    let body = fetch_profile(&client, &handle.base_url, ws.path()).await;
    assert_eq!(body["markdown"].as_str(), Some(""));
    assert!(body["error"].is_null(), "unexpected error: {body}");
}
