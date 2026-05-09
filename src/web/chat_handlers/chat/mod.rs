//! `POST /chat`、`/chat/stream`、`/chat/approval`、`/chat/branch`。

mod async_chat;
mod builtin_skills;
mod enqueue;
pub(super) mod stream;
mod turn_build;

pub(crate) use async_chat::{chat_async_handler, chat_job_status_handler};
pub(crate) use enqueue::prepare_json_chat_enqueue;
pub(crate) use stream::chat_stream_handler;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode};
use log::debug;

use super::parse::{normalize_approval_session_id, normalize_client_conversation_id};
use crate::conversation_store::SaveConversationOutcome;
use crate::types::{CommandApprovalDecision, filter_messages_for_web_client_snapshot};
use crate::web::app_state::AppState;
use crate::web::http_types::chat::{
    ApiError, ChatApprovalRequestBody, ChatApprovalResponseBody, ChatBranchRequestBody,
    ChatBranchResponseBody, ChatRequestBody, ChatResponseBody, ConversationMessagesQuery,
    ConversationMessagesResponseBody,
};

use builtin_skills::run_web_builtin_command;
use enqueue::{enqueue_and_wait_json_chat, parse_chat_request_for_enqueue};

pub(crate) async fn chat_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Json<ChatResponseBody>, (StatusCode, Json<ApiError>)> {
    let parsed = parse_chat_request_for_enqueue(&state, &body).await?;
    if let Some(reply) = run_web_builtin_command(&state, parsed.user_trim.as_str()).await {
        return Ok(Json(ChatResponseBody {
            reply,
            conversation_id: parsed.conversation_id,
            conversation_revision: None,
        }));
    }
    let cid = parsed.conversation_id.clone();
    let (messages, _) = enqueue_and_wait_json_chat(state.clone(), peer, &headers, parsed).await?;
    let reply = messages
        .last()
        .and_then(|m| crate::types::message_content_as_str(&m.content))
        .unwrap_or("")
        .to_string();
    let conversation_revision = state
        .load_conversation_seed(&cid)
        .await
        .and_then(|s| s.expected_revision);
    Ok(Json(ChatResponseBody {
        reply,
        conversation_id: cid,
        conversation_revision,
    }))
}

pub(crate) async fn chat_approval_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatApprovalRequestBody>,
) -> Result<Json<ChatApprovalResponseBody>, (StatusCode, Json<ApiError>)> {
    let session_id = normalize_approval_session_id(&body.approval_session_id).ok_or((
        StatusCode::BAD_REQUEST,
        Json(ApiError {
            code: "INVALID_APPROVAL_SESSION_ID",
            message: "approval_session_id 非法或为空".to_string(),
            reason_code: None,
        }),
    ))?;
    let decision = match body.decision.trim().to_ascii_lowercase().as_str() {
        "deny" => CommandApprovalDecision::Deny,
        "allow_once" => CommandApprovalDecision::AllowOnce,
        "allow_always" => CommandApprovalDecision::AllowAlways,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_APPROVAL_DECISION",
                    message: "decision 仅支持 deny / allow_once / allow_always".to_string(),
                    reason_code: None,
                }),
            ));
        }
    };
    let tx = {
        let guard = state.aux.approval_sessions.read().await;
        guard.get(&session_id).map(|s| s.tx.clone())
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ApiError {
            code: "APPROVAL_SESSION_NOT_FOUND",
            message: "审批会话不存在或已结束".to_string(),
            reason_code: None,
        }),
    ))?;
    if tx.send(decision).await.is_err() {
        debug!(
            target: "crabmate::sse_mpsc",
            "approval decision mpsc send failed: session_id={} receiver dropped",
            session_id
        );
        state
            .aux
            .approval_sessions
            .write()
            .await
            .remove(&session_id);
        return Err((
            StatusCode::GONE,
            Json(ApiError {
                code: "APPROVAL_SESSION_CLOSED",
                message: "审批会话已关闭".to_string(),
                reason_code: None,
            }),
        ));
    }
    Ok(Json(ChatApprovalResponseBody { ok: true }))
}

/// 将会话历史截断到前 N 条消息（`keep_message_count`），**同一** `conversation_id` 下继续对话。
pub(crate) async fn chat_branch_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatBranchRequestBody>,
) -> Result<Json<ChatBranchResponseBody>, (StatusCode, Json<ApiError>)> {
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
    let ord = usize::try_from(body.before_user_ordinal).unwrap_or(usize::MAX);
    let seed = state.load_conversation_seed(&cid).await;
    let Some(seed) = seed else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "CONVERSATION_NOT_FOUND",
                message: "会话不存在或已过期".to_string(),
                reason_code: None,
            }),
        ));
    };
    let Some(exp) = seed.expected_revision else {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_REVISION_UNKNOWN",
                message: "无法分支：缺少 revision 信息".to_string(),
                reason_code: None,
            }),
        ));
    };
    if exp != body.expected_revision {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_CONFLICT",
                message: "revision 不匹配，请刷新后重试".to_string(),
                reason_code: None,
            }),
        ));
    }
    match state
        .truncate_conversation_before_user_ordinal_if_revision(
            cid.clone(),
            ord,
            body.expected_revision,
        )
        .await
    {
        SaveConversationOutcome::Saved => {}
        SaveConversationOutcome::Conflict => {
            return Err((
                StatusCode::CONFLICT,
                Json(ApiError {
                    code: "CONVERSATION_CONFLICT",
                    message: "会话已被其他请求更新或 revision 不匹配".to_string(),
                    reason_code: None,
                }),
            ));
        }
    }
    let new_rev = state
        .load_conversation_seed(&cid)
        .await
        .and_then(|s| s.expected_revision)
        .unwrap_or(body.expected_revision);
    Ok(Json(ChatBranchResponseBody {
        ok: true,
        revision: new_rev,
    }))
}

/// 只读拉取服务端已持久化的会话消息与 revision（Web 刷新后与 `conversation_id` 对齐）。
pub(crate) async fn conversation_messages_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ConversationMessagesQuery>,
) -> Result<Json<ConversationMessagesResponseBody>, (StatusCode, Json<ApiError>)> {
    let conversation_id =
        normalize_client_conversation_id(Some(&q.conversation_id)).map_err(|msg| {
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
    let Some(seed) = state.load_conversation_seed(&cid).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "CONVERSATION_NOT_FOUND",
                message: "会话不存在或已过期".to_string(),
                reason_code: None,
            }),
        ));
    };
    let Some(revision) = seed.expected_revision else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError {
                code: "CONVERSATION_NOT_FOUND",
                message: "会话不存在或已过期".to_string(),
                reason_code: None,
            }),
        ));
    };
    let messages = filter_messages_for_web_client_snapshot(&seed.messages);
    let active_agent_role = seed
        .persisted_active_agent_role
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Ok(Json(ConversationMessagesResponseBody {
        conversation_id: cid,
        revision,
        active_agent_role,
        messages,
    }))
}
