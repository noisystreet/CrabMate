//! `POST /config/session/conversation-store`：进程内切换 Web 会话存储（内存 / SQLite）。

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;

use super::super::app_state::AppState;
use crate::web::http_types::chat::{
    ApiError, SessionConversationStoreRequestBody, SessionConversationStoreResponseBody,
};

pub(crate) async fn session_conversation_store_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<SessionConversationStoreRequestBody>,
) -> Result<Json<SessionConversationStoreResponseBody>, (StatusCode, Json<ApiError>)> {
    match state.set_web_conversation_store_sqlite(body.sqlite).await {
        Ok(()) => {
            let msg = if body.sqlite {
                "已切换为 SQLite 会话存储（当前进程）。重启 serve 后仍以配置文件为准。"
            } else {
                "已切换为内存会话存储（当前进程）；未写入配置文件。重启 serve 后仍以配置文件为准。"
            };
            Ok(Json(SessionConversationStoreResponseBody {
                ok: true,
                message: msg.to_string(),
            }))
        }
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "SESSION_STORE_SWITCH_FAILED",
                message: e,
                reason_code: None,
            }),
        )),
    }
}
