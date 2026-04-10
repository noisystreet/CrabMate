//! `POST /chat`、`/chat/stream`、`/chat/approval`、`/chat/branch`。

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, StreamExt};
use log::{debug, error, info, warn};
use tokio::sync::mpsc;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use super::super::app_state::{AppState, ConversationTurnSeed};
use super::conflict::conversation_conflict_api_error;
use super::parse::{
    ensure_bearer_api_key_for_chat, normalize_agent_role, normalize_approval_session_id,
    normalize_chat_image_urls, normalize_client_conversation_id, parse_client_llm_override,
    parse_optional_chat_temperature, parse_seed_override_from_body,
};
use crate::agent_memory::load_memory_snippet;
use crate::agent_role_turn::maybe_apply_mid_session_agent_role_switch;
use crate::chat_job_queue;
use crate::conversation_store::SaveConversationOutcome;
use crate::conversation_turn_bootstrap::{
    compose_new_conversation_messages, first_turn_project_context_user_message,
};
use crate::redact;
use crate::types::{CommandApprovalDecision, Message, message_user_with_images};
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use crate::web::http_types::chat::{
    ApiError, ChatApprovalRequestBody, ChatApprovalResponseBody, ChatBranchRequestBody,
    ChatBranchResponseBody, ChatRequestBody, ChatResponseBody,
};

fn sse_event_with_id(seq: u64, data: String) -> Result<Event, Infallible> {
    Ok(Event::default().id(seq.to_string()).data(data))
}

fn reject_if_client_sse_protocol_invalid(
    client_sse_protocol: Option<u8>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let Some(v) = client_sse_protocol else {
        return Ok(());
    };
    if v == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_SSE_CLIENT_PROTOCOL",
                message: "client_sse_protocol 非法（须为 1～255）".to_string(),
            }),
        ));
    }
    if v > crate::sse::protocol::SSE_PROTOCOL_VERSION {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "SSE_CLIENT_TOO_NEW",
                message: "客户端声明的 SSE 协议版本高于服务端，请升级服务器或更换匹配的前端构建"
                    .to_string(),
            }),
        ));
    }
    Ok(())
}

fn parse_last_event_id(headers: &HeaderMap) -> Option<u64> {
    let raw = headers.get(axum::http::HeaderName::from_static("last-event-id"))?;
    let s = raw.to_str().ok()?.trim();
    if s.is_empty() {
        return None;
    }
    s.parse::<u64>().ok()
}

async fn build_messages_for_turn(
    state: &Arc<AppState>,
    conversation_id: &str,
    user_msg: &str,
    image_urls: &[String],
    agent_role: Option<&str>,
) -> Result<ConversationTurnSeed, String> {
    let last_user = if image_urls.is_empty() {
        Message::user_only(user_msg.to_string())
    } else {
        message_user_with_images(user_msg, image_urls)
    };
    if let Some(mut seed) = state.load_conversation_seed(conversation_id).await {
        let persisted = seed.persisted_active_agent_role.clone();
        {
            let cfg = state.cfg.read().await;
            if let Some(id) = agent_role.map(str::trim).filter(|s| !s.is_empty()) {
                cfg.system_prompt_for_new_conversation(Some(id))
                    .map_err(|e| e.to_string())?;
            }
            maybe_apply_mid_session_agent_role_switch(
                &cfg,
                &mut seed.messages,
                persisted.as_deref(),
                agent_role,
            )?;
        }
        seed.messages.push(last_user);
        return Ok(seed);
    }
    // 先取工作区路径，再读 `cfg`，避免在持有 `cfg` 读锁时调用 `effective_workspace_path`（其内部再次 `cfg.read` 会死锁）。
    let root_str = state.effective_workspace_path().await;
    let cfg = state.cfg.read().await;
    let system_for_turn = cfg
        .system_prompt_for_new_conversation(agent_role)
        .map_err(|e| e.to_string())?
        .to_string();
    let system_for_turn = crate::tool_stats::augment_system_prompt(&system_for_turn, &cfg);
    let root = std::path::PathBuf::from(root_str);
    let memory_snippet = if cfg.agent_memory_file_enabled {
        load_memory_snippet(
            &root,
            cfg.agent_memory_file.as_str(),
            cfg.agent_memory_file_max_chars,
        )
    } else {
        None
    };

    let combined =
        first_turn_project_context_user_message(root.as_path(), &cfg, memory_snippet).await;
    let messages = compose_new_conversation_messages(&system_for_turn, combined, Some(last_user));
    Ok(ConversationTurnSeed {
        messages,
        expected_revision: None,
        persisted_active_agent_role: None,
    })
}

pub(crate) async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Json<ChatResponseBody>, (StatusCode, Json<ApiError>)> {
    let image_urls = normalize_chat_image_urls(&body.image_urls).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_IMAGE_URLS",
                message: e,
            }),
        )
    })?;
    let user_trim = body.message.trim();
    if user_trim.is_empty() && image_urls.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空（若仅发图须至少附带一张图片）".to_string(),
            }),
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: e,
                }),
            )
        })?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
            }),
        )
    })?;
    let temperature_override = parse_optional_chat_temperature(body.temperature).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_TEMPERATURE",
                message: e,
            }),
        )
    })?;
    let seed_override =
        parse_seed_override_from_body(body.seed, body.seed_policy).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_SEED",
                    message: e,
                }),
            )
        })?;
    let llm_override = parse_client_llm_override(body.client_llm).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CLIENT_LLM",
                message: e,
            }),
        )
    })?;
    ensure_bearer_api_key_for_chat(&state, &llm_override).await?;
    let work_dir_pb = std::path::PathBuf::from(state.effective_workspace_path().await);
    let msg = {
        let cfg = state.cfg.read().await;
        expand_at_file_refs_in_user_message(user_trim, work_dir_pb.as_path(), &cfg).map_err(
            |e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: "INVALID_AT_FILE_REF",
                        message: e,
                    }),
                )
            },
        )?
    };
    let turn_seed = build_messages_for_turn(
        &state,
        &conversation_id,
        &msg,
        &image_urls,
        agent_role.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
            }),
        )
    })?;
    let work_dir_str = work_dir_pb.to_string_lossy().to_string();
    let work_dir = work_dir_str.clone();
    let workspace_is_set = state.workspace_is_set().await;
    let job_id = state.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    debug!(
        target: "crabmate",
        "chat json 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(&msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat json 任务入队 job_id={}", job_id);
    state
        .chat_queue
        .try_submit_json(chat_job_queue::JsonSubmitParams {
            job_id,
            state: state.clone(),
            conversation_id: conversation_id.clone(),
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            request_agent_role: agent_role.clone(),
            persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
            work_dir: std::path::PathBuf::from(work_dir),
            workspace_is_set,
            temperature_override,
            seed_override,
            llm_override,
            reply_tx,
        })
        .map_err(|e| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ApiError {
                    code: "QUEUE_FULL",
                    message: format!(
                        "对话任务队列已满（最多等待 {} 个），请稍后重试",
                        e.max_pending
                    ),
                }),
            )
        })?;
    let messages = reply_rx
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: "INTERNAL_ERROR",
                    message: "对话任务被取消或内部错误".to_string(),
                }),
            )
        })?
        .map_err(|e| {
            if e.trim() == "CONVERSATION_CONFLICT" {
                return conversation_conflict_api_error();
            }
            error!(
                target: "crabmate",
                "chat_handler 队列任务失败 job_id={} error={}",
                job_id,
                e
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: "INTERNAL_ERROR",
                    message: "对话失败，请稍后重试".to_string(),
                }),
            )
        })?;
    let reply = messages
        .last()
        .and_then(|m| crate::types::message_content_as_str(&m.content))
        .unwrap_or("")
        .to_string();
    let conversation_revision = state
        .load_conversation_seed(&conversation_id)
        .await
        .and_then(|s| s.expected_revision);
    Ok(Json(ChatResponseBody {
        reply,
        conversation_id,
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
                }),
            ));
        }
    };
    let tx = {
        let guard = state.approval_sessions.read().await;
        guard.get(&session_id).cloned()
    }
    .ok_or((
        StatusCode::NOT_FOUND,
        Json(ApiError {
            code: "APPROVAL_SESSION_NOT_FOUND",
            message: "审批会话不存在或已结束".to_string(),
        }),
    ))?;
    if tx.send(decision).await.is_err() {
        debug!(
            target: "crabmate::sse_mpsc",
            "approval decision mpsc send failed: session_id={} receiver dropped",
            session_id
        );
        state.approval_sessions.write().await.remove(&session_id);
        return Err((
            StatusCode::GONE,
            Json(ApiError {
                code: "APPROVAL_SESSION_CLOSED",
                message: "审批会话已关闭".to_string(),
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
                }),
            )
        })?;
    let Some(cid) = conversation_id else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CONVERSATION_ID",
                message: "conversation_id 不能为空".to_string(),
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
            }),
        ));
    };
    let Some(exp) = seed.expected_revision else {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_REVISION_UNKNOWN",
                message: "无法分支：缺少 revision 信息".to_string(),
            }),
        ));
    };
    if exp != body.expected_revision {
        return Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                code: "CONVERSATION_CONFLICT",
                message: "revision 不匹配，请刷新后重试".to_string(),
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

/// 流式 chat：返回 SSE，每个 event 的 **`id`** 为单调序号（断线重连与 **`Last-Event-ID`** / **`stream_resume`**），`data` 为控制面 JSON 或正文 delta。
pub(crate) async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<ChatRequestBody>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let resume = body.stream_resume.as_ref();
    let image_urls = normalize_chat_image_urls(&body.image_urls).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_IMAGE_URLS",
                message: e,
            }),
        )
    })?;
    let user_trim = body.message.trim();
    if user_trim.is_empty() && resume.is_none() && image_urls.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空（若仅发图须至少附带一张图片）".to_string(),
            }),
        ));
    }
    reject_if_client_sse_protocol_invalid(body.client_sse_protocol)?;
    let conversation_id = normalize_client_conversation_id(body.conversation_id.as_deref())
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_CONVERSATION_ID",
                    message: e,
                }),
            )
        })?
        .unwrap_or_else(|| state.next_conversation_id());
    let agent_role = normalize_agent_role(body.agent_role.as_deref()).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
            }),
        )
    })?;
    let temperature_override = parse_optional_chat_temperature(body.temperature).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_TEMPERATURE",
                message: e,
            }),
        )
    })?;
    let seed_override =
        parse_seed_override_from_body(body.seed, body.seed_policy).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_SEED",
                    message: e,
                }),
            )
        })?;
    let llm_override = parse_client_llm_override(body.client_llm).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_CLIENT_LLM",
                message: e,
            }),
        )
    })?;
    ensure_bearer_api_key_for_chat(&state, &llm_override).await?;

    if let Some(sr) = resume {
        let job_id = sr.job_id;
        if !state.sse_stream_hub.has_job(job_id) {
            return Err((
                StatusCode::GONE,
                Json(ApiError {
                    code: "STREAM_JOB_GONE",
                    message: "流式任务已结束或不在本进程内存中，无法重连".to_string(),
                }),
            ));
        }
        let after_header = parse_last_event_id(&headers).unwrap_or(0);
        let after_body = sr.after_seq.unwrap_or(0);
        let after_seq = after_header.max(after_body);
        let Some(sub) = state.sse_stream_hub.subscribe(job_id) else {
            return Err((
                StatusCode::GONE,
                Json(ApiError {
                    code: "STREAM_JOB_GONE",
                    message: "流式任务已结束或不在本进程内存中，无法重连".to_string(),
                }),
            ));
        };
        let replay = state
            .sse_stream_hub
            .replay_after(job_id, after_seq)
            .unwrap_or_default();
        let max_replayed = replay.last().map(|(s, _)| *s).unwrap_or(after_seq);
        info!(
            target: "crabmate",
            "chat stream 断线重连 job_id={} after_seq={} replayed={}",
            job_id,
            after_seq,
            replay.len()
        );
        let replay_st = stream::iter(replay).map(|(seq, data)| sse_event_with_id(seq, data));
        let live_st = BroadcastStream::new(sub).filter_map(move |item| {
            std::future::ready(match item {
                Ok((seq, data)) if seq > max_replayed => Some(sse_event_with_id(seq, data)),
                Ok(_) => None,
                Err(BroadcastStreamRecvError::Lagged(n)) => {
                    warn!(
                        target: "crabmate",
                        "chat stream 重连 broadcast lag job_id={} skipped={}",
                        job_id,
                        n
                    );
                    None
                }
            })
        });
        let merged = replay_st.chain(live_st);
        let mut resp = Sse::new(merged)
            .keep_alive(KeepAlive::default())
            .into_response();
        if let Ok(v) = HeaderValue::from_str(&job_id.to_string()) {
            resp.headers_mut().insert("x-stream-job-id", v);
        }
        if let Ok(v) = HeaderValue::from_str(&conversation_id) {
            resp.headers_mut().insert("x-conversation-id", v);
        }
        return Ok(resp);
    }

    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
    let msg = {
        let cfg = state.cfg.read().await;
        expand_at_file_refs_in_user_message(user_trim, work_dir.as_path(), &cfg).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    code: "INVALID_AT_FILE_REF",
                    message: e,
                }),
            )
        })?
    };
    let turn_seed = build_messages_for_turn(
        &state,
        &conversation_id,
        &msg,
        &image_urls,
        agent_role.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
            }),
        )
    })?;
    let workspace_is_set = state.workspace_is_set().await;
    let approval_session_id = match body.approval_session_id.as_deref() {
        Some(v) => Some(normalize_approval_session_id(v).ok_or((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_APPROVAL_SESSION_ID",
                message: "approval_session_id 非法或为空".to_string(),
            }),
        ))?),
        None => None,
    };
    let mut web_approval_session = None;
    if let Some(session_id) = approval_session_id.as_ref() {
        let (approval_tx, approval_rx) = mpsc::channel::<CommandApprovalDecision>(8);
        state
            .approval_sessions
            .write()
            .await
            .insert(session_id.clone(), approval_tx);
        web_approval_session = Some(chat_job_queue::WebApprovalSession {
            session_id: session_id.clone(),
            approval_rx,
        });
    }
    let job_id = state.chat_queue.next_job_id();
    let (tx, rx) = mpsc::channel::<(u64, String)>(1024);
    debug!(
        target: "crabmate",
        "chat stream 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(&msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat stream 任务入队 job_id={}", job_id);
    if let Err(e) = state
        .chat_queue
        .try_submit_stream(chat_job_queue::StreamSubmitParams {
            job_id,
            state: state.clone(),
            conversation_id: conversation_id.clone(),
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            request_agent_role: agent_role.clone(),
            persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
            work_dir,
            workspace_is_set,
            temperature_override,
            seed_override,
            llm_override,
            stream_event_tx: tx,
            web_approval_session,
        })
    {
        if let Some(session_id) = approval_session_id {
            state.approval_sessions.write().await.remove(&session_id);
        }
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ApiError {
                code: "QUEUE_FULL",
                message: format!(
                    "对话任务队列已满（最多等待 {} 个），请稍后重试",
                    e.max_pending
                ),
            }),
        ));
    }
    let stream = ReceiverStream::new(rx).map(|(seq, data)| sse_event_with_id(seq, data));
    let mut resp = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&conversation_id) {
        resp.headers_mut().insert("x-conversation-id", v);
    }
    if let Ok(v) = HeaderValue::from_str(&job_id.to_string()) {
        resp.headers_mut().insert("x-stream-job-id", v);
    }
    Ok(resp)
}
