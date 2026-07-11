//! E2E 专用夹具路由：仅在环境变量 **`CM_E2E_FIXTURES=1`**（或 **`true`**）时挂载。
//!
//! **不得**在生产部署中启用；供 Victauri E2E 等写入可分页的持久化会话，无需真实 LLM。

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use serde::Deserialize;

use crate::AppState;
use crate::types::{Message, MessageContent};
use crate::web::http_types::chat::ApiError;
use crate::web::normalize_client_conversation_id;

#[derive(Deserialize)]
struct E2eSeedMessage {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct E2eSeedConversationBody {
    conversation_id: String,
    messages: Vec<E2eSeedMessage>,
    /// 为 true 时若 id 已存在则先删再写（仅 E2E）。
    #[serde(default)]
    replace: bool,
}

fn e2e_fixtures_enabled() -> bool {
    std::env::var("CM_E2E_FIXTURES")
        .ok()
        .is_some_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

/// 非 E2E 时返回 `None`，`build_app` 不挂载。
pub(crate) fn router() -> Option<Router<Arc<AppState>>> {
    if !e2e_fixtures_enabled() {
        return None;
    }
    Some(Router::new().route(
        "/e2e/fixtures/conversation",
        post(seed_conversation_handler),
    ))
}

async fn seed_conversation_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<E2eSeedConversationBody>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiError>)> {
    let conversation_id =
        normalize_client_conversation_id(Some(&body.conversation_id)).map_err(|msg| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: msg,
                    reason_code: None,
                }),
            )
        })?;
    let Some(cid) = conversation_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CONVERSATION_ID",
                message: "conversation_id 不能为空".to_string(),
                reason_code: None,
            }),
        ));
    };
    if body.messages.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_BODY",
                message: "messages 不能为空".to_string(),
                reason_code: None,
            }),
        ));
    }
    let messages: Vec<Message> = body
        .messages
        .into_iter()
        .map(|m| Message {
            role: m.role,
            content: Some(MessageContent::from(m.content.as_str())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        })
        .collect();
    if body.replace {
        state.delete_conversation_record(&cid).await;
    }
    let outcome = state
        .save_conversation_messages_if_revision(cid.clone(), messages, None, None)
        .await;
    match outcome {
        crate::SaveConversationOutcome::Saved => Ok(StatusCode::NO_CONTENT),
        crate::SaveConversationOutcome::Conflict => Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_EXISTS",
                message: "会话已存在；可设 replace=true".to_string(),
                reason_code: None,
            }),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::e2e_fixtures_enabled;

    #[test]
    fn enabled_when_env_one() {
        assert!(
            matches!(
                std::env::var("CM_E2E_FIXTURES").ok().as_deref(),
                Some("1") | Some("true") | Some("TRUE")
            ) || !e2e_fixtures_enabled()
        );
    }
}
