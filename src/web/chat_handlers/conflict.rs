//! 会话 revision 冲突时 HTTP / SSE 统一文案与载荷。

use axum::Json;
use axum::http::StatusCode;

use crate::web::http_types::chat::ApiError;

/// 与 SSE `code`、JSON `ApiError.code` 一致。
pub(crate) const CONVERSATION_CONFLICT_CODE: &str = "CONVERSATION_CONFLICT";

/// 面向用户的冲突说明（HTTP body 与 SSE `error` 一致）。
pub(crate) const CONVERSATION_CONFLICT_MESSAGE: &str = "会话已被其他请求更新，请重试本次提问";

pub(crate) fn conversation_conflict_http_response() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::CONFLICT,
        Json(ApiError {
            code: CONVERSATION_CONFLICT_CODE,
            message: CONVERSATION_CONFLICT_MESSAGE.to_string(),
        }),
    )
}

pub(crate) fn conversation_conflict_sse_line() -> String {
    crate::sse::encode_message(crate::sse::SsePayload::Error(crate::sse::SseErrorBody {
        error: CONVERSATION_CONFLICT_MESSAGE.to_string(),
        code: Some(CONVERSATION_CONFLICT_CODE.to_string()),
        reason_code: None,
        turn_id: None,
        sub_phase: None,
    }))
}

pub(super) fn conversation_conflict_api_error() -> (StatusCode, Json<ApiError>) {
    conversation_conflict_http_response()
}
