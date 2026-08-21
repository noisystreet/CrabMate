//! `GET /uploads/{filename}` 轻量 HTTP 冒烟：真实 axum，不启 LLM、不挂 Bearer。

use crate::test_serve::start_test_serve;

const MIN_PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

#[tokio::test]
async fn get_uploads_http_smoke_survives_workspace_switch() {
    let handle = start_test_serve(None).await;
    let name = "u_smoke.png";
    std::fs::write(handle.uploads_dir.join(name), MIN_PNG).expect("write upload");

    let client = loopback_http_client();
    let url = format!("{}/uploads/{name}", handle.base_url);
    let ok = client.get(&url).send().await.expect("GET upload");
    assert_eq!(ok.status(), reqwest::StatusCode::OK);
    assert_eq!(ok.bytes().await.expect("body").as_ref(), MIN_PNG);

    let other = tempfile::tempdir().expect("other ws");
    let set = client
        .post(format!("{}/workspace", handle.base_url))
        .json(&serde_json::json!({ "path": other.path() }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(
        set.status().is_success(),
        "set workspace: {}",
        set.status()
    );

    let still = client.get(&url).send().await.expect("GET after switch");
    assert_eq!(
        still.status(),
        reqwest::StatusCode::OK,
        "uploads dir must not follow workspace_override"
    );

    let missing = client
        .get(format!("{}/uploads/missing.png", handle.base_url))
        .send()
        .await
        .expect("GET missing");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    let bad = client
        .get(format!("{}/uploads/..png", handle.base_url))
        .send()
        .await
        .expect("GET bad name");
    assert_eq!(bad.status(), reqwest::StatusCode::BAD_REQUEST);
}
