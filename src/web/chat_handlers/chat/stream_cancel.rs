//! `POST /chat/stream/{job_id}/cancel`：用户停止流式回合（与 abort SSE 正交，以便 `stream_resume`）。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;

use crate::chat_job_queue::ChatJobQueue;
use crate::cm_internal::tool_jobs::ToolJobRegistry;
use crate::web::app_state_facets::WebChatTurnAppFacet;
use crate::web::http_types::chat::ApiError;

/// 成功取消（任务仍在队列登记表中）。
#[derive(Debug, Serialize)]
pub(crate) struct ChatStreamCancelResponseBody {
    pub job_id: u64,
    pub cancelled: bool,
    /// 本回合 `source_turn_job_id` 下被请求取消的后台 `run_command` 任务数。
    pub background_tools_cancelled: usize,
}

fn apply_stream_cancel(
    chat_queue: &ChatJobQueue,
    tool_job_registry: &ToolJobRegistry,
    job_id: u64,
) -> Result<ChatStreamCancelResponseBody, ApiError> {
    if !chat_queue.request_stream_cancel(job_id) {
        return Err(ApiError::new(
            crate::cm_api_contract::error_codes::STREAM_JOB_GONE,
            "流式任务已结束或不在本进程内存中，无法取消",
        ));
    }
    let background_tools_cancelled =
        tool_job_registry.cancel_non_terminal_for_source_turn(job_id);
    Ok(ChatStreamCancelResponseBody {
        job_id,
        cancelled: true,
        background_tools_cancelled,
    })
}

pub(crate) async fn chat_stream_cancel_handler(
    State(state): State<WebChatTurnAppFacet>,
    Path(job_id): Path<u64>,
) -> Result<Json<ChatStreamCancelResponseBody>, (StatusCode, Json<ApiError>)> {
    apply_stream_cancel(&state.chat.chat_queue, &state.tool_job_registry, job_id)
        .map(Json)
        .map_err(|e| (StatusCode::GONE, Json(e)))
}

#[cfg(test)]
mod tests {
    use super::{ChatStreamCancelResponseBody, apply_stream_cancel};
    use crate::chat_job_queue::ChatJobQueue;
    use crate::cm_internal::tool_jobs::{JobLimits, JobSpawn, ToolJobRegistry};
    use crate::test_serve::start_test_serve;
    use std::path::PathBuf;
    use std::time::Duration;

    fn tool_limits() -> JobLimits {
        JobLimits {
            max_concurrent: 2,
            max_queued: 4,
            ttl: Duration::from_secs(3600),
            grace: Duration::from_secs(60),
            max_entries: 16,
            output_buffer_bytes: 262_144,
        }
    }

    fn spawn_true() -> JobSpawn {
        JobSpawn {
            program: "true".to_string(),
            args: Vec::new(),
            cwd: PathBuf::from("/"),
            extra_env: Vec::new(),
            wall: Duration::from_secs(10),
            max_output_len: 1024,
        }
    }

    #[test]
    fn cancel_body_serializes_stable_keys() {
        let v = serde_json::to_value(ChatStreamCancelResponseBody {
            job_id: 9,
            cancelled: true,
            background_tools_cancelled: 2,
        })
        .expect("json");
        assert_eq!(v["job_id"], 9);
        assert_eq!(v["cancelled"], true);
        assert_eq!(v["background_tools_cancelled"], 2);
    }

    #[tokio::test]
    async fn apply_cancel_gone_when_unregistered() {
        let q = ChatJobQueue::new(1, 1);
        let reg = ToolJobRegistry::new(tool_limits());
        let err = apply_stream_cancel(&q, &reg, 7).expect_err("gone");
        assert_eq!(err.code, crate::cm_api_contract::error_codes::STREAM_JOB_GONE);
    }

    #[tokio::test]
    async fn apply_cancel_sets_flag_and_cancels_matching_tool_jobs() {
        let q = ChatJobQueue::new(1, 1);
        let flag = q.register_stream_cancel(7);
        let reg = ToolJobRegistry::new(tool_limits());
        let keep = reg
            .register(
                PathBuf::from("/ws"),
                Some(1),
                spawn_true(),
                r#"{"command":"true"}"#.to_string(),
            )
            .expect("keep");
        let stop = reg
            .register(
                PathBuf::from("/ws"),
                Some(7),
                spawn_true(),
                r#"{"command":"true"}"#.to_string(),
            )
            .expect("stop");
        let body = apply_stream_cancel(&q, &reg, 7).unwrap_or_else(|e| {
            panic!("expected Ok, got code={}", e.code)
        });
        assert!(body.cancelled);
        assert_eq!(body.job_id, 7);
        assert_eq!(body.background_tools_cancelled, 1);
        assert!(flag.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            reg.get(&stop).expect("stop").status,
            crate::cm_internal::tool_jobs::JobStatus::Cancelled
        );
        assert_eq!(
            reg.get(&keep).expect("keep").status,
            crate::cm_internal::tool_jobs::JobStatus::Queued
        );
    }

    fn loopback_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("reqwest client")
    }

    #[tokio::test]
    async fn http_cancel_unknown_job_is_gone() {
        let handle = start_test_serve(None).await;
        let resp = loopback_http_client()
            .post(format!("{}/chat/stream/999/cancel", handle.base_url))
            .send()
            .await
            .expect("POST cancel");
        assert_eq!(resp.status(), reqwest::StatusCode::GONE);
        let v: serde_json::Value = resp.json().await.expect("json");
        assert_eq!(v["code"], "STREAM_JOB_GONE");
    }

    #[tokio::test]
    async fn http_cancel_registered_job_is_ok() {
        let handle = start_test_serve(None).await;
        let _flag = handle.chat_queue.register_stream_cancel(42);
        let resp = loopback_http_client()
            .post(format!("{}/chat/stream/42/cancel", handle.base_url))
            .send()
            .await
            .expect("POST cancel");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let v: serde_json::Value = resp.json().await.expect("json");
        assert_eq!(v["job_id"], 42);
        assert_eq!(v["cancelled"], true);
        assert_eq!(v["background_tools_cancelled"], 0);
    }
}
