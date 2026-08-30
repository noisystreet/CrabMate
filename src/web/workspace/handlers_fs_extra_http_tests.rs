//! 目录 zip、文件 move 与批量文件删除的轻量 HTTP 冒烟。

use crate::test_serve::start_test_serve;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

async fn set_workspace(client: &reqwest::Client, base: &str, path: &std::path::Path) {
    let set = client
        .post(format!("{base}/workspace"))
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(set.status().is_success(), "set workspace");
}

#[tokio::test]
async fn workspace_dir_archive_http_smoke() {
    let root = tempfile::tempdir().expect("tempdir");
    let dir = root.path().join("notes");
    std::fs::create_dir(&dir).expect("mkdir");
    std::fs::write(dir.join("a.txt"), b"hello").expect("write");

    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    set_workspace(&client, &handle.base_url, root.path()).await;

    let as_file = client
        .get(format!("{}/workspace/dir/archive", handle.base_url))
        .query(&[("path", "notes/a.txt")])
        .send()
        .await
        .expect("archive file");
    assert_eq!(as_file.status(), reqwest::StatusCode::BAD_REQUEST);

    let trav = client
        .get(format!("{}/workspace/dir/archive", handle.base_url))
        .query(&[("path", "../notes")])
        .send()
        .await
        .expect("archive trav");
    assert_eq!(trav.status(), reqwest::StatusCode::BAD_REQUEST);

    let ok = client
        .get(format!("{}/workspace/dir/archive", handle.base_url))
        .query(&[("path", "notes")])
        .send()
        .await
        .expect("archive dir");
    assert_eq!(ok.status(), reqwest::StatusCode::OK);
    let ctype = ok
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert_eq!(ctype, "application/zip");
    let bytes = ok.bytes().await.expect("zip body");
    assert!(
        bytes.starts_with(b"PK"),
        "zip magic, got {:?}",
        &bytes[..4.min(bytes.len())]
    );

    let missing = client
        .get(format!("{}/workspace/dir/archive", handle.base_url))
        .query(&[("path", "no_such_dir")])
        .send()
        .await
        .expect("archive missing");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn workspace_file_move_http_smoke() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(root.path().join("a.txt"), b"x").expect("write");
    std::fs::write(root.path().join("exists.txt"), b"y").expect("write exists");

    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    set_workspace(&client, &handle.base_url, root.path()).await;

    let moved = client
        .post(format!("{}/workspace/file/move", handle.base_url))
        .json(&serde_json::json!({ "from": "a.txt", "to": "b.txt" }))
        .send()
        .await
        .expect("move");
    assert_eq!(moved.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(!root.path().join("a.txt").exists());
    assert_eq!(std::fs::read(root.path().join("b.txt")).expect("read b"), b"x");

    let conflict = client
        .post(format!("{}/workspace/file/move", handle.base_url))
        .json(&serde_json::json!({ "from": "b.txt", "to": "exists.txt" }))
        .send()
        .await
        .expect("conflict");
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);

    let trav = client
        .post(format!("{}/workspace/file/move", handle.base_url))
        .json(&serde_json::json!({ "from": "../a.txt", "to": "c.txt" }))
        .send()
        .await
        .expect("trav");
    assert_eq!(trav.status(), reqwest::StatusCode::BAD_REQUEST);

    let missing = client
        .post(format!("{}/workspace/file/move", handle.base_url))
        .json(&serde_json::json!({ "from": "gone.txt", "to": "z.txt" }))
        .send()
        .await
        .expect("missing");
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    let bad_cid = client
        .post(format!("{}/workspace/file/move", handle.base_url))
        .json(&serde_json::json!({
            "from": "b.txt",
            "to": "c.txt",
            "conversation_id": "not valid"
        }))
        .send()
        .await
        .expect("bad cid");
    assert_eq!(bad_cid.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(root.path().join("b.txt").exists(), "move must not run after bad conversation_id");

    let over = client
        .post(format!("{}/workspace/file/move", handle.base_url))
        .json(&serde_json::json!({
            "from": "b.txt",
            "to": "exists.txt",
            "overwrite": true
        }))
        .send()
        .await
        .expect("overwrite");
    assert_eq!(over.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(!root.path().join("b.txt").exists());
    assert_eq!(
        std::fs::read(root.path().join("exists.txt")).expect("read exists"),
        b"x"
    );
}

#[tokio::test]
async fn workspace_files_delete_batch_http_smoke() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(root.path().join("a.txt"), b"a").expect("write a");
    std::fs::write(root.path().join("b.txt"), b"b").expect("write b");
    std::fs::create_dir(root.path().join("d")).expect("mkdir d");
    std::fs::write(root.path().join("d").join("c.txt"), b"c").expect("write c");

    let handle = start_test_serve(None).await;
    let client = loopback_http_client();
    set_workspace(&client, &handle.base_url, root.path()).await;

    // 目录在校验阶段整批拒绝：a.txt 不应被删。
    let with_dir = client
        .delete(format!("{}/workspace/file", handle.base_url))
        .query(&[("paths", "a.txt,d")])
        .send()
        .await
        .expect("batch with dir");
    assert_eq!(with_dir.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = with_dir.json().await.expect("json");
    assert!(
        body["error"].as_str().unwrap_or("").contains("不支持删除目录"),
        "{body}"
    );
    assert!(
        root.path().join("a.txt").exists(),
        "整批校验失败时不应删除任何文件"
    );

    // 批量成功：三个文件都删。
    let ok = client
        .delete(format!("{}/workspace/file", handle.base_url))
        .query(&[("paths", "a.txt,b.txt,d/c.txt")])
        .send()
        .await
        .expect("batch delete");
    assert_eq!(ok.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = ok.json().await.expect("json");
    let deleted = body["deleted"].as_array().expect("deleted array");
    assert_eq!(deleted.len(), 3, "{body}");
    assert!(body["failed"].is_null(), "{body}");
    assert!(!root.path().join("a.txt").exists());
    assert!(!root.path().join("b.txt").exists());
    assert!(!root.path().join("d").join("c.txt").exists());

    // 任一缺失 → 整批拒绝：x.txt 不应被删。
    std::fs::write(root.path().join("x.txt"), b"x").expect("write x");
    let missing = client
        .delete(format!("{}/workspace/file", handle.base_url))
        .query(&[("paths", "x.txt,missing.txt")])
        .send()
        .await
        .expect("batch missing");
    assert_eq!(missing.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = missing.json().await.expect("json");
    assert!(
        body["error"]
            .as_str()
            .map(|s| s.contains("路径无法解析"))
            .unwrap_or(false),
        "{body}"
    );
    assert!(
        root.path().join("x.txt").exists(),
        "整批校验失败时不应删除任何文件"
    );

    // 超过批量上限 → 报错。
    let many = (0..33)
        .map(|i| format!("f{i}.txt"))
        .collect::<Vec<_>>()
        .join(",");
    let over = client
        .delete(format!("{}/workspace/file", handle.base_url))
        .query(&[("paths", many.as_str())])
        .send()
        .await
        .expect("batch over cap");
    assert_eq!(over.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = over.json().await.expect("json");
    assert!(
        body["error"]
            .as_str()
            .map(|s| s.contains("最多删除 32"))
            .unwrap_or(false),
        "{body}"
    );
}
