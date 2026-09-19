//! `DELETE /conversation/{conversation_id}` 轻量 HTTP 冒烟：真实 axum，不启 LLM、不挂 Bearer。
//!
//! 会话「存在 → 删除后不可读」的语义由 `web::app_state` 单测覆盖（内存与 SQLite 双后端）。

use crate::test_serve::start_test_serve;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

#[tokio::test]
async fn delete_conversation_http_smoke_is_idempotent_and_validates_id() {
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();

    // 不存在（也从未存在）→ 204（幂等）。
    let missing = client
        .delete(format!("{}/conversation/missing-conv", handle.base_url))
        .send()
        .await
        .expect("DELETE missing");
    assert_eq!(missing.status(), reqwest::StatusCode::NO_CONTENT);

    let again = client
        .delete(format!("{}/conversation/missing-conv", handle.base_url))
        .send()
        .await
        .expect("DELETE missing again");
    assert_eq!(again.status(), reqwest::StatusCode::NO_CONTENT);

    // 非法字符 → 400 INVALID_CONVERSATION_ID。
    let bad = client
        .delete(format!("{}/conversation/bad!id", handle.base_url))
        .send()
        .await
        .expect("DELETE bad id");
    assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);
    let body: serde_json::Value = bad.json().await.expect("json");
    assert_eq!(body["code"], "INVALID_CONVERSATION_ID");

    // 超长（> 128）→ 400。
    let too_long = client
        .delete(format!(
            "{}/conversation/{}",
            handle.base_url,
            "a".repeat(200)
        ))
        .send()
        .await
        .expect("DELETE long id");
    assert_eq!(too_long.status(), reqwest::StatusCode::BAD_REQUEST);
}
