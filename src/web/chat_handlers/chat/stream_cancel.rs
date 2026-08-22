//! `POST /chat/stream/{job_id}/cancel`：用户停止流式回合（与 abort SSE 正交，以便 `stream_resume`）。

use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Serialize;

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

pub(crate) async fn chat_stream_cancel_handler(
    State(state): State<WebChatTurnAppFacet>,
    Path(job_id): Path<u64>,
) -> Result<Json<ChatStreamCancelResponseBody>, (StatusCode, Json<ApiError>)> {
    if !state.chat.chat_queue.request_stream_cancel(job_id) {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                crate::cm_api_contract::error_codes::STREAM_JOB_GONE,
                "流式任务已结束或不在本进程内存中，无法取消",
            )),
        ));
    }
    let background_tools_cancelled = state
        .tool_job_registry
        .cancel_non_terminal_for_source_turn(job_id);
    Ok(Json(ChatStreamCancelResponseBody {
        job_id,
        cancelled: true,
        background_tools_cancelled,
    }))
}

#[cfg(test)]
mod tests {
    use super::ChatStreamCancelResponseBody;

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
}
