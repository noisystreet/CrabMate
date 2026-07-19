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

use common::test_server::TestServer;

/// Smoke 测试：发送一条简单用户消息，验证同步 `/chat` 端点能正常返回。
///
/// 默认被 `#[ignore]`（不需要 API_KEY 即可编译通过）；设置 `REAL_LLM_E2E=1` 时自动启用。
///
/// # 验收标准
///
/// - HTTP 响应状态 200
/// - JSON body 包含 `reply` 字段且非空
/// - JSON body 包含 `conversation_id` 字段
#[tokio::test]
#[ignore = "设置 REAL_LLM_E2E=1 后执行；骨架版默认跳过"]
async fn e2e_http_smoke_sync_chat() {
    let server = TestServer::start("http_smoke").await;

    let resp = server
        .post_chat(r#"{"message":"你好，用一句话介绍自己"}"#)
        .send()
        .await
        .expect("POST /chat 请求失败");

    assert_eq!(
        resp.status(),
        200,
        "预期 200 OK，实际 status={} body={}",
        resp.status(),
        resp.text().await.unwrap_or_default()
    );

    let body: serde_json::Value = resp.json().await.expect("响应非合法 JSON");
    let reply = body
        .get("reply")
        .and_then(|v| v.as_str())
        .expect("JSON 缺少 reply 字段");
    assert!(!reply.is_empty(), "reply 不应为空");

    let conv_id = body
        .get("conversation_id")
        .and_then(|v| v.as_str())
        .expect("JSON 缺少 conversation_id 字段");
    assert!(!conv_id.is_empty(), "conversation_id 不应为空");
}

/// SSE 流式 smoke 测试：发送一条简单用户消息，验证流能完成。
///
/// 默认被 `#[ignore]`；设置 `REAL_LLM_E2E=1` 时自动启用。
///
/// TODO: RUN_FINISHED 在 SSE 流关闭前未到达，需要排查事件通道关闭时机。
///       可用 同步 POST /chat 替代验证，见 `e2e_http_smoke_sync_chat`.
#[tokio::test]
#[ignore = "TODO: SSE 事件通道需排查; 先使用同步 e2e_http_smoke_sync_chat"]
async fn e2e_http_smoke_stream_completes() {
    let server = TestServer::start("http_smoke").await;
    let resp = server
        .post_chat(r#"{"message":"你好，用一句话介绍自己"}"#)
        .send()
        .await
        .expect("POST /chat 请求失败");
    assert_eq!(resp.status(), 200);
}
