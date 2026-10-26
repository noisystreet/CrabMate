//! `POST /conversation/state` 的轻量 HTTP 冒烟：真实 axum 路由 + 真实配置，不启 LLM、不挂 Bearer。
//!
//! 断言只依赖**契约形状**与**错误码**，不钉具体等级值：`start_test_serve` 会合并本机
//! `~/.config/crabmate/config.toml`，把 `permission_level` 写死会让测试在开发机上翻车。

use crate::test_serve::start_test_serve;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

/// 先造一个已落库会话，再对它切档（`POST /conversation/state` 要求会话已存在）。
///
/// 直接走 `AppState` 而不是 E2E 夹具：夹具要 `CM_E2E_FIXTURES=1`，本测试不该依赖环境变量。
async fn seed_conversation(handle: &crate::test_serve::TestServeHandle, id: &str) {
    let message = crate::types::Message {
        role: "user".to_string(),
        content: Some(crate::types::MessageContent::from("seed")),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let outcome = handle
        .state
        .conversation
        .save_conversation_messages_if_revision(
            id.to_string(),
            vec![message],
            None,
            None,
            None,
            None,
            Vec::new(),
        )
        .await;
    assert_eq!(
        outcome,
        crate::SaveConversationOutcome::Saved,
        "造会话失败"
    );
}

#[tokio::test]
async fn conversation_state_writes_level_and_returns_projection() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let cid = "conv_state_smoke_1";
    seed_conversation(&handle, cid).await;

    let resp = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "conversation_id": cid, "permission_level": "workspace" }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("state json");

    assert_eq!(body["conversation_id"], cid);
    assert_eq!(body["active_permission_level"], "workspace");
    // 意图落库后，投影的 `level_source` 必须是 `persisted`——这正是「空闲态切换立刻可见」的判据。
    assert_eq!(body["permission"]["level_source"], "persisted");
    assert!(
        body["permission"]["available"].is_array(),
        "available 必须是数组"
    );

    // 再 `GET /permission` 读回同一档：写端点与只读投影不能两套判据。
    let got: serde_json::Value = client
        .get(format!("{}/permission", handle.base_url))
        .query(&[("conversation_id", cid)])
        .send()
        .await
        .expect("GET /permission")
        .json()
        .await
        .expect("permission json");
    assert_eq!(got["level_source"], "persisted");
    assert_eq!(got["level"], body["permission"]["level"]);
}

#[tokio::test]
async fn conversation_state_is_idempotent_for_same_value() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let cid = "conv_state_smoke_2";
    seed_conversation(&handle, cid).await;

    for _ in 0..2 {
        let resp = client
            .post(format!("{}/conversation/state", handle.base_url))
            .json(&serde_json::json!({ "conversation_id": cid, "session_mode": "plan" }))
            .send()
            .await
            .expect("POST /conversation/state");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let body: serde_json::Value = resp.json().await.expect("state json");
        assert_eq!(body["active_session_mode"], "plan");
    }
}

#[tokio::test]
async fn conversation_state_rejects_bad_input() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();

    // 空 patch：三个字段都没给。
    let empty = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "conversation_id": "conv_state_smoke_3" }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(empty.status(), reqwest::StatusCode::BAD_REQUEST);
    let empty_body: serde_json::Value = empty.json().await.expect("error json");
    assert_eq!(empty_body["code"], "EMPTY_CONVERSATION_STATE_PATCH");

    // 未知等级：必须 400，不能静默回落。
    let bad_level = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({
            "conversation_id": "conv_state_smoke_3",
            "permission_level": "root"
        }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(bad_level.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_level_body: serde_json::Value = bad_level.json().await.expect("error json");
    assert_eq!(bad_level_body["code"], "INVALID_PERMISSION_LEVEL");

    // 非法 conversation_id（含 `/`）。
    let bad_id = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "conversation_id": "conv/bad", "session_mode": "act" }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(bad_id.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_id_body: serde_json::Value = bad_id.json().await.expect("error json");
    assert_eq!(bad_id_body["code"], "INVALID_CONVERSATION_ID");
}

#[tokio::test]
async fn conversation_state_does_not_bump_revision_nor_get_clobbered_by_turn_save() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let cid = "conv_state_smoke_4";
    seed_conversation(&handle, cid).await;
    let revision_before = fetch_revision(&client, &handle.base_url, cid).await;

    // 空闲态切档：只改意图列，`revision` 必须原地不动（否则进行中回合的 CAS 落盘会变 Conflict）。
    let resp = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "conversation_id": cid, "permission_level": "workspace" }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    assert_eq!(
        fetch_revision(&client, &handle.base_url, cid).await,
        revision_before,
        "切档不得 bump revision"
    );

    // 模拟「下一轮请求没带 permission_level」的落盘：`None` = 保持该列，
    // 不能用回合开始时的旧快照把用户刚切的档改回去。
    let turn_save = handle
        .state
        .conversation
        .save_conversation_messages_if_revision(
            cid.to_string(),
            vec![crate::types::Message::system_only("s".to_string())],
            None,
            None,
            None,
            Some(revision_before),
            Vec::new(),
        )
        .await;
    assert_eq!(turn_save, crate::SaveConversationOutcome::Saved);

    let level = client
        .get(format!("{}/permission", handle.base_url))
        .query(&[("conversation_id", cid)])
        .send()
        .await
        .expect("GET /permission")
        .json::<serde_json::Value>()
        .await
        .expect("permission json");
    assert_eq!(level["level_source"], "persisted", "回合结束不得回退切档");
}

/// `GET /conversation/messages` 的 `revision`（读取侧唯一稳定口径）。
async fn fetch_revision(client: &reqwest::Client, base_url: &str, cid: &str) -> u64 {
    let body: serde_json::Value = client
        .get(format!("{base_url}/conversation/messages"))
        .query(&[("conversation_id", cid)])
        .send()
        .await
        .expect("GET /conversation/messages")
        .json()
        .await
        .expect("messages json");
    body["revision"].as_u64().expect("revision u64")
}

#[tokio::test]
async fn conversation_state_reports_missing_conversation_as_404() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let resp = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({
            "conversation_id": "conv_no_such_state_1",
            "permission_level": "read"
        }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(resp.status(), reqwest::StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json().await.expect("error json");
    assert_eq!(body["code"], "CONVERSATION_NOT_FOUND");
}

/// 未知 `session_mode` / 未知 `agent_role` 都必须 400：静默回落会让「顶部开关没生效」无从排查。
#[tokio::test]
async fn conversation_state_rejects_bad_session_mode_and_unknown_role() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let cid = "conv_state_smoke_5";
    seed_conversation(&handle, cid).await;

    let bad_mode = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "conversation_id": cid, "session_mode": "yolo" }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(bad_mode.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_mode_body: serde_json::Value = bad_mode.json().await.expect("error json");
    assert_eq!(bad_mode_body["code"], "INVALID_SESSION_MODE");

    // 没人会配这个名字的角色；若本机 config.toml 恰好同名，本用例会在 200 上失败并提示改 id。
    let bad_role = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({
            "conversation_id": cid,
            "agent_role": "__crabmate_no_such_role__"
        }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(bad_role.status(), reqwest::StatusCode::BAD_REQUEST);
    let bad_role_body: serde_json::Value = bad_role.json().await.expect("error json");
    assert_eq!(bad_role_body["code"], "INVALID_AGENT_ROLE");
}

/// 缺 `conversation_id` 必须是本端点的 **400 `INVALID_CONVERSATION_ID`**，而不是 axum 的 422：
/// 契约层用 `Option<String>` 接住缺字段，避免「参数错误」长出第二种错误体。
#[tokio::test]
async fn conversation_state_missing_conversation_id_is_400() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let resp = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "permission_level": "read" }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(resp.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = resp.json().await.expect("error json");
    assert_eq!(body["code"], "INVALID_CONVERSATION_ID");
}

/// 配置里**有**命名角色时，`agent_role` 也要能落库并原样回显（等级 / 模式已有各自的用例）。
///
/// 角色从启动配置里取而非写死：`start_test_serve` 会合并本机 `~/.config/crabmate/config.toml`，
/// 钉死某个 id 会让测试在别的开发机上翻车；配置里一个角色都没有时直接跳过。
#[tokio::test]
async fn conversation_state_writes_agent_role_when_configured() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let cid = "conv_state_smoke_6";
    seed_conversation(&handle, cid).await;

    let role = {
        let cfg = handle.state.http.cfg.read().await;
        cfg.roles_prompts.agent_roles.keys().next().cloned()
    };
    let Some(role) = role else {
        return;
    };

    let resp = client
        .post(format!("{}/conversation/state", handle.base_url))
        .json(&serde_json::json!({ "conversation_id": cid, "agent_role": role }))
        .send()
        .await
        .expect("POST /conversation/state");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("state json");
    assert_eq!(body["active_agent_role"], role.as_str());
}
