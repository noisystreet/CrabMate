//! `GET /workspace/file/raw` 轻量 HTTP 冒烟：真实 axum + 临时工作区，不启 LLM、不挂 Bearer。

use crate::test_serve::start_test_serve;

/// 67 字节 1×1 PNG，避免额外夹具文件。
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

async fn get_raw(client: &reqwest::Client, base: &str, path: &str) -> reqwest::Response {
    client
        .get(format!("{base}/workspace/file/raw"))
        .query(&[("path", path)])
        .send()
        .await
        .expect("GET /workspace/file/raw")
}

#[tokio::test]
async fn workspace_file_raw_http_smoke() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(root.path().join("ok.png"), MIN_PNG).expect("write png");
    std::fs::write(root.path().join("x.svg"), b"<svg xmlns='http://www.w3.org/2000/svg'/>")
        .expect("write svg");

    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let set = client
        .post(format!("{}/workspace", handle.base_url))
        .json(&serde_json::json!({ "path": root.path() }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(
        set.status().is_success(),
        "set workspace: {} {}",
        set.status(),
        set.text().await.unwrap_or_default()
    );

    let png = get_raw(&client, &handle.base_url, "ok.png").await;
    assert_eq!(png.status(), reqwest::StatusCode::OK);
    let ctype = png
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(ctype, "image/png");
    let nosniff = png
        .headers()
        .get("x-content-type-options")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(nosniff, "nosniff");
    assert_eq!(png.bytes().await.expect("png body").as_ref(), MIN_PNG);

    let svg = get_raw(&client, &handle.base_url, "x.svg").await;
    assert_eq!(svg.status(), reqwest::StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let trav = get_raw(&client, &handle.base_url, "../ok.png").await;
    assert_eq!(trav.status(), reqwest::StatusCode::BAD_REQUEST);

    let missing = get_raw(&client, &handle.base_url, "missing.png").await;
    assert!(
        missing.status().is_client_error(),
        "missing file should 4xx, got {}",
        missing.status()
    );
}

async fn put_raw(
    client: &reqwest::Client,
    base: &str,
    path: &str,
    body: &[u8],
    create_only: bool,
) -> reqwest::Response {
    let mut req = client
        .put(format!("{base}/workspace/file/raw"))
        .query(&[("path", path)]);
    if create_only {
        req = req.query(&[("create_only", "true")]);
    }
    req.header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
        .body(body.to_vec())
        .send()
        .await
        .expect("PUT /workspace/file/raw")
}

#[tokio::test]
async fn workspace_file_raw_put_http_smoke() {
    let root = tempfile::tempdir().expect("tempdir");
    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    let set = client
        .post(format!("{}/workspace", handle.base_url))
        .json(&serde_json::json!({ "path": root.path() }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(set.status().is_success(), "set workspace");

    let bin = [0u8, 1, 0xff, 0x00, b'A'];
    let created = put_raw(&client, &handle.base_url, "a.bin", &bin, true).await;
    assert_eq!(created.status(), reqwest::StatusCode::NO_CONTENT);
    let on_disk = std::fs::read(root.path().join("a.bin")).expect("read a.bin");
    assert_eq!(on_disk.as_slice(), bin.as_slice());

    let exists = put_raw(&client, &handle.base_url, "a.bin", b"x", true).await;
    assert_eq!(exists.status(), reqwest::StatusCode::CONFLICT);

    let overwrite = put_raw(&client, &handle.base_url, "a.bin", b"ok", false).await;
    assert_eq!(overwrite.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        std::fs::read(root.path().join("a.bin")).expect("read overwrite"),
        b"ok"
    );

    let trav = put_raw(&client, &handle.base_url, "../x.bin", b"x", false).await;
    assert_eq!(trav.status(), reqwest::StatusCode::BAD_REQUEST);

    let dotted = put_raw(&client, &handle.base_url, "foo..bar.bin", b"ab", true).await;
    assert_eq!(dotted.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        std::fs::read(root.path().join("foo..bar.bin")).expect("read foo..bar.bin"),
        b"ab"
    );
}
