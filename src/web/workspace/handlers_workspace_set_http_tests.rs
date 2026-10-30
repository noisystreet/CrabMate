//! `POST /workspace` 的「一实例一工作区」轻量 HTTP 冒烟：真实 axum + 两个临时目录。
//!
//! 钉住四件事（Issue #1183 §2 ① / §3.1）：
//! 1. 待绑定（`None`）首次绑定成功；
//! 2. 同值重放幂等 200；
//! 3. 异值 409 + 可机器处理的 `WORKSPACE_ALREADY_SET`，原绑定不变；
//! 4. **空串复位放行**，且复位后能换到新路径。
//!
//! 第 4 条是回归钉：早期实现把空串也算作「不同路径」，已绑定实例连 TUI 的
//! `/workspace off`（`workspace_set_payload` 把 `off`/`default` 映射成空串）都会被
//! 409 挡死，「换工作区」的两步第一步就断。

use serde_json::{Value, json};

use crate::test_serve::start_test_serve;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

/// `POST /workspace` → `(status, json)`；非 JSON body 直接判定为失败（本接口恒回 JSON）。
async fn post_workspace(client: &reqwest::Client, base: &str, body: &Value) -> (u16, Value) {
    let resp = client
        .post(format!("{base}/workspace"))
        .json(body)
        .send()
        .await
        .expect("POST /workspace");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("workspace set body");
    let val = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("非法 JSON: {e}; body={text}"))
    };
    (status, val)
}

#[tokio::test]
async fn workspace_set_is_one_bind_per_instance() {
    let dir_a = tempfile::tempdir().expect("tempdir a");
    let dir_b = tempfile::tempdir().expect("tempdir b");
    let a = std::fs::canonicalize(dir_a.path())
        .expect("canonicalize a")
        .to_str()
        .expect("utf8 path a")
        .to_string();
    let b = std::fs::canonicalize(dir_b.path())
        .expect("canonicalize b")
        .to_str()
        .expect("utf8 path b")
        .to_string();

    let handle = start_test_serve(None).await;
    let client = loopback_http_client();

    // 1. 待绑定（`None`）→ 首次绑定 A 成功。
    let (st, v) = post_workspace(&client, &handle.base_url, &json!({ "path": a })).await;
    assert_eq!(st, 200, "首次绑定应成功: {v}");
    assert_eq!(v["ok"], true, "{v}");

    // 2. 同值重放：幂等 200，回显同一路径。
    let (st, v) = post_workspace(&client, &handle.base_url, &json!({ "path": a })).await;
    assert_eq!(st, 200, "同值应幂等: {v}");
    assert_eq!(v["path"].as_str(), Some(a.as_str()), "{v}");

    // 3. 异值：409 + code，原绑定不变。
    let (st, v) = post_workspace(&client, &handle.base_url, &json!({ "path": b })).await;
    assert_eq!(st, 409, "异值应 409: {v}");
    assert_eq!(v["ok"], false, "{v}");
    assert_eq!(v["code"], "WORKSPACE_ALREADY_SET", "{v}");

    // 4a. 空串 = 显式复位（设计稿 §3.1），必须放行。
    let (st, v) = post_workspace(&client, &handle.base_url, &json!({ "path": "" })).await;
    assert_eq!(st, 200, "空串复位应放行: {v}");
    assert_eq!(v["path"], "", "{v}");

    // 4b. 复位后可以换到 B：这是「换工作区」的两步路径。
    let (st, v) = post_workspace(&client, &handle.base_url, &json!({ "path": b })).await;
    assert_eq!(st, 200, "复位后应能切到新路径: {v}");
    assert_eq!(v["ok"], true, "{v}");
}

/// `GET /workspace` → `(status, json)`。
async fn get_workspace(client: &reqwest::Client, base: &str) -> (u16, Value) {
    let resp = client
        .get(format!("{base}/workspace"))
        .send()
        .await
        .expect("GET /workspace");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("workspace body");
    let val = serde_json::from_str(&text).unwrap_or_else(|e| panic!("非法 JSON: {e}; body={text}"));
    (status, val)
}

/// 待绑定实例在首次绑定前的**可观察态**：`GET /workspace` 报未设置，绑定后立刻反映新作用域。
///
/// 待绑定的完整语义是「`workspace_override` 为 `None` + 工具面回 `WORKSPACE_NOT_SET`」
/// （#1183 §2 ③）。若这里把 `run_command_working_dir` 当成 `path` 回显，client 会以为
/// 实例已绑定某个目录，而工具调用同时被拒——两处说法互相矛盾，排查时无从下手。
#[tokio::test]
async fn pending_workspace_reports_unset_then_binds() {
    let dir_a = tempfile::tempdir().expect("tempdir a");
    let a = std::fs::canonicalize(dir_a.path())
        .expect("canonicalize a")
        .to_str()
        .expect("utf8 path a")
        .to_string();

    // `start_test_serve` 以 `workspace_override: None` 启动，正是 ctl spawn 出的待绑定态。
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();

    // 1. 未设置不是错误态：`path` 为空、`error` 为 null，且不借默认目录充数。
    let (st, v) = get_workspace(&client, &handle.base_url).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(
        v["path"], "",
        "待绑定时 path 必须为空，而不是回落到 run_command_working_dir: {v}"
    );
    assert_eq!(v["entries"].as_array().map(Vec::len), Some(0), "{v}");
    assert!(v["error"].is_null(), "未设置不是错误态: {v}");

    // 2. client 绑定后，同一实例的 GET 立即反映新作用域（不需要重启）。
    let (st, v) = post_workspace(&client, &handle.base_url, &json!({ "path": a.clone() })).await;
    assert_eq!(st, 200, "首次绑定应成功: {v}");
    let (st, v) = get_workspace(&client, &handle.base_url).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["path"].as_str(), Some(a.as_str()), "{v}");
}
