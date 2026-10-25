//! `GET /permission` 轻量 HTTP 冒烟：真实 axum 路由 + 真实配置，不启 LLM、不挂 Bearer。
//!
//! 断言只依赖**契约形状**与**错误码**，不钉具体等级值：`start_test_serve` 会合并本机
//! `~/.config/crabmate/config.toml`，把 `permission_level` / 上限写死会让测试在开发机上翻车。

use crate::test_serve::start_test_serve;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

const LEVELS: [&str; 5] = ["chat", "read", "workspace", "approved", "unrestricted"];

fn level_index(raw: &str) -> usize {
    LEVELS
        .iter()
        .position(|l| *l == raw)
        .unwrap_or_else(|| panic!("未知等级 {raw}（不是 chat/read/workspace/approved/unrestricted）"))
}

#[tokio::test]
async fn permission_http_smoke_reports_shape_and_bounds() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let resp = client
        .get(format!("{}/permission", handle.base_url))
        .send()
        .await
        .expect("GET /permission");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("permission json");

    let level = body["level"].as_str().expect("level string");
    level_index(level);
    let ceiling = body["ceiling"].as_str().expect("ceiling string");
    let ceiling_idx = level_index(ceiling);

    // 生效等级永不超过上限。
    assert!(
        level_index(level) <= ceiling_idx,
        "level={level} 高于 ceiling={ceiling}"
    );

    let available: Vec<&str> = body["available"]
        .as_array()
        .expect("available array")
        .iter()
        .map(|v| v.as_str().expect("available entries are strings"))
        .collect();
    assert!(!available.is_empty(), "available 不能为空");
    // 自低到高且以一级为步长（选择器直接渲染，不能跳档）。
    for pair in available.windows(2) {
        assert_eq!(
            level_index(pair[1]),
            level_index(pair[0]) + 1,
            "available 必须是连续区间：{available:?}"
        );
    }
    // 选项集不得越上限，且当前生效等级必须在选项集内。
    assert!(
        level_index(available[available.len() - 1]) <= ceiling_idx,
        "available 越过 ceiling：{available:?} vs {ceiling}"
    );
    assert!(
        available.contains(&level),
        "level={level} 不在 available={available:?} 内"
    );

    let ceiling_source = body["ceiling_source"].as_str().expect("ceiling_source");
    assert!(
        matches!(
            ceiling_source,
            "config" | "derived_loopback" | "derived_non_loopback"
        ),
        "未知 ceiling_source={ceiling_source}"
    );
    // 会话级等级写入（S5b）之前，来源恒为配置。
    assert_eq!(body["level_source"], "config");
    assert!(body["clamped_by"].is_array(), "clamped_by 必须是数组（可空）");

    for axis in [
        "tool_visibility",
        "external_tools",
        "path_scope",
        "command_scope",
        "net_scope",
        "approval",
    ] {
        assert!(
            body["caps"][axis].as_str().is_some_and(|s| !s.is_empty()),
            "caps.{axis} 缺失或为空"
        );
    }
}

#[tokio::test]
async fn permission_http_rejects_unknown_agent_role() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let resp = client
        .get(format!("{}/permission", handle.base_url))
        .query(&[("agent_role", "__no_such_role__")])
        .send()
        .await
        .expect("GET /permission?agent_role=…");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.expect("error json");
    assert_eq!(body["code"], "INVALID_AGENT_ROLE");
}

#[tokio::test]
async fn permission_http_reports_missing_conversation_as_404() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let resp = client
        .get(format!("{}/permission", handle.base_url))
        .query(&[("conversation_id", "conv_no_such_session_1")])
        .send()
        .await
        .expect("GET /permission?conversation_id=…");
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.expect("error json");
    assert_eq!(body["code"], "CONVERSATION_NOT_FOUND");

    // 非法 id（含 `/`）在参数校验阶段就拒绝，不落到「查库」。
    let bad = client
        .get(format!("{}/permission", handle.base_url))
        .query(&[("conversation_id", "conv/bad")])
        .send()
        .await
        .expect("GET /permission?conversation_id=conv/bad");
    assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_body: serde_json::Value = bad.json().await.expect("error json");
    assert_eq!(bad_body["code"], "INVALID_CONVERSATION_ID");
}
