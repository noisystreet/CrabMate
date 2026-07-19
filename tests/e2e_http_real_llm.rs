//! HTTP/SSE 协议级 e2e 真实 LLM 测试（Layer 1）。
//!
//! 骨架版：启动真实 axum 实例 + 默认 HTTP LLM 后端。
//!
//! 运行模式（环境变量）：
//! - **默认（无 `REAL_LLM_E2E`）**：`#[ignore]` 跳过所有用例
//! - `REAL_LLM_E2E=1`：真实 LLM 后端，不录制
//! - `REAL_LLM_E2E=1 CM_E2E_RECORD=1`：真实 LLM 后端 + 录制
//!
//! 需要 feature **`web`**（默认已启用）。

// Feature gate: test_serve 需要 `feature = "web"`
#![cfg(feature = "web")]

mod common;

use common::sse_stream::SseEventStream;
use common::test_server::TestServer;

/// Smoke 测试：发送一条简单用户消息，验证 SSE 流能正常完成（`content` + `done` 事件）。
///
/// 默认被 `#[ignore]`（不需要 API_KEY 即可编译通过）；设置 `REAL_LLM_E2E=1` 时自动启用。
///
/// # 验收标准
///
/// - SSE 流中出现至少一个 `content_delta` 或 `content` 事件
/// - SSE 流中出现 `done` 或 `finish` 事件
/// - 无 `error` 事件
#[tokio::test]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；骨架版默认跳过"]
async fn e2e_http_smoke_stream_completes() {
    let server = TestServer::start("http_smoke").await;

    let resp = server
        .post_chat_stream(r#"{"messages":[{"role":"user","content":"你好，用一句话介绍自己"}]}"#)
        .send()
        .await
        .expect("POST /chat/stream 请求失败");

    assert_eq!(
        resp.status(),
        200,
        "预期 200 OK，实际 status={} body={}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    // 重新获取响应（text() 消耗了 body）
    let resp = server
        .post_chat_stream(r#"{"messages":[{"role":"user","content":"你好，用一句话介绍自己"}]}"#)
        .send()
        .await
        .expect("POST /chat/stream 请求失败");

    let mut stream = SseEventStream::new(resp);
    let mut saw_content = false;
    let mut saw_done = false;
    let mut event_types = Vec::new();

    while let Some(ev) = stream.next_event().await {
        event_types.push(ev.event.clone());
        match ev.event.as_str() {
            "content_delta" | "content" => saw_content = true,
            "done" | "finish" => {
                saw_done = true;
                break;
            }
            "error" => {
                panic!("收到 error SSE 事件: data={}", ev.data);
            }
            _ => {}
        }
    }

    // 失败时落盘事件序列
    if !saw_content || !saw_done {
        let _ = std::fs::write(
            server.artifacts_dir.join("sse_events.txt"),
            format!("{:?}", event_types),
        );
    }

    assert!(saw_content, "未收到 content 事件; events={:?}", event_types);
    assert!(saw_done, "未收到 done 事件; events={:?}", event_types);
}
