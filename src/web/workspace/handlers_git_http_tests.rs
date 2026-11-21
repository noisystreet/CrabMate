//! `GET /workspace/git` 的 HTTP 冒烟：四态可区分、脏摘要随改动变化、缓存与 `refresh`。
//!
//! 骨架照 `handlers_profile_http_tests.rs`：`start_test_serve_with_config` + 临时目录。
//! 细粒度解析（前缀裁剪、字段偏移、异常形状）由 `git_projection` 的单测覆盖，这里只验接线。

use std::path::Path;

use crate::test_serve::start_test_serve_with_config;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

fn cfg_default() -> crate::config::AgentConfig {
    crate::config::load_config(None).expect("embed default")
}

/// 在 `dir` 内跑 git（带本地身份，避免依赖宿主机 git 配置）。
fn git(dir: &Path, args: &[&str]) {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-c")
        .arg("user.name=crabmate-test")
        .arg("-c")
        .arg("user.email=test@example.com");
    cmd.args(args).current_dir(dir);
    let out = cmd.output().expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
}

/// `POST /workspace` 设根，返回 `GET /workspace/git` 的响应体。
async fn setup_and_get(
    client: &reqwest::Client,
    base: &str,
    workspace: &Path,
    query: &str,
) -> serde_json::Value {
    let set = client
        .post(format!("{base}/workspace"))
        .json(&serde_json::json!({ "path": workspace }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(set.status().is_success(), "set workspace: {:?}", set);
    let resp = client
        .get(format!("{base}/workspace/git{query}"))
        .send()
        .await
        .expect("GET /workspace/git");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    resp.json().await.expect("git json body")
}

#[tokio::test]
async fn clean_repo_reports_branch_and_not_dirty() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["head"]["kind"], "branch", "{body}");
    assert_eq!(body["dirty"], false, "{body}");
    assert_eq!(body["source"], "probe", "{body}");
    assert!(body.get("reason_code").is_none(), "{body}");
}

#[tokio::test]
async fn new_file_makes_worktree_dirty() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    std::fs::write(ws.path().join("a.txt"), "x").expect("write");
    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["dirty"], true, "{body}");
    assert_eq!(body["counts"]["untracked"], 1, "{body}");
    assert_eq!(body["counts"]["total"], 1, "{body}");
}

#[tokio::test]
async fn detached_head_is_distinguishable() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    git(ws.path(), &["checkout", "-q", "--detach"]);
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["head"]["kind"], "detached", "{body}");
    assert!(
        body["head"].get("name").is_none(),
        "detached 不应出现 name 键：{body}"
    );
}

#[tokio::test]
async fn non_repo_directory_reports_not_a_repo() {
    let ws = tempfile::tempdir().expect("tempdir");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["state"], "not_a_repo", "{body}");
    assert!(body.get("head").is_none(), "{body}");
    assert!(body.get("counts").is_none(), "{body}");
}

#[tokio::test]
async fn unset_workspace_reports_failed_workspace_not_set() {
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let resp = client
        .get(format!("{}/workspace/git", handle.base_url))
        .send()
        .await
        .expect("GET /workspace/git");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("body");
    assert_eq!(body["state"], "failed", "{body}");
    assert_eq!(body["reason_code"], "WORKSPACE_NOT_SET", "{body}");
}

#[tokio::test]
async fn second_request_within_ttl_is_served_from_cache() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let first = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(first["source"], "probe", "{first}");
    let second = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(second["source"], "cache", "{second}");
    assert_eq!(
        second["computed_at_unix"], first["computed_at_unix"],
        "命中缓存不应推进探测时刻：{second}"
    );
}

#[tokio::test]
async fn refresh_bypasses_cache() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let _ = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?refresh=true").await;
    assert_eq!(body["source"], "probe", "{body}");
}

#[tokio::test]
async fn records_outside_workspace_subdir_are_not_counted() {
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    let sub = repo.path().join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    // 工作区根 = 仓库子目录；改动落在根外。
    std::fs::write(repo.path().join("outside.txt"), "x").expect("write outside");
    let body = setup_and_get(&client, &handle.base_url, &sub, "").await;
    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["dirty"], false, "根外改动不应计入：{body}");

    // 根内改动才计入。
    std::fs::write(sub.join("inside.txt"), "x").expect("write inside");
    let body = setup_and_get(&client, &handle.base_url, &sub, "?refresh=true").await;
    assert_eq!(body["dirty"], true, "{body}");
    assert_eq!(body["counts"]["untracked"], 1, "{body}");
}
