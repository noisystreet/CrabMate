//! SSE `chat/stream`：事件序号与 handler。

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::stream::{self, StreamExt};
use log::{debug, info, warn};
use tokio::sync::mpsc;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::chat_job_queue;
use crate::types::CommandApprovalDecision;
use crate::user_message_file_refs::expand_at_file_refs_in_user_message;
use crate::web::http_types::chat::{ApiError, ChatRequestBody};

use crate::clarification_questionnaire::merge_user_text_with_clarification_answers;
use crate::redact;
use crate::web::app_state::AppState;
use crate::web::audit;

use super::super::parse::{ensure_bearer_api_key_for_chat, normalize_approval_session_id};
use super::builtin_skills::run_web_builtin_command;
use super::turn_build::{build_messages_for_turn, parse_chat_stream_request, parse_last_event_id};

pub(super) fn sse_event_with_id(seq: u64, data: String) -> Result<Event, Infallible> {
    Ok(Event::default().id(seq.to_string()).data(data))
}

/// 流式 chat：返回 SSE，每个 event 的 **`id`** 为单调序号（断线重连与 **`Last-Event-ID`** / **`stream_resume`**），`data` 为控制面 JSON 或正文 delta。
pub(crate) async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let p = parse_chat_stream_request(&state, &body)?;
    let resume = p.resume.as_ref();
    ensure_bearer_api_key_for_chat(&state, &p.llm_override).await?;
    if let Some(reply) = run_web_builtin_command(&state, p.user_trim.as_str()).await {
        let stream =
            stream::iter(vec![(1_u64, reply)]).map(|(seq, data)| sse_event_with_id(seq, data));
        let mut resp = Sse::new(stream)
            .keep_alive(KeepAlive::default())
            .into_response();
        if let Ok(v) = HeaderValue::from_str(&p.conversation_id) {
            resp.headers_mut().insert("x-conversation-id", v);
        }
        return Ok(resp);
    }

    if let Some(sr) = resume {
        let job_id = sr.job_id;
        if !state.sse_stream_hub.has_job(job_id) {
            return Err((
                StatusCode::GONE,
                Json(ApiError {
                    code: "STREAM_JOB_GONE",
                    message: "流式任务已结束或不在本进程内存中，无法重连".to_string(),
                    reason_code: None,
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
                    reason_code: None,
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
        if let Ok(v) = HeaderValue::from_str(&p.conversation_id) {
            resp.headers_mut().insert("x-conversation-id", v);
        }
        return Ok(resp);
    }

    let eff_ws_raw = state.effective_workspace_path().await;
    let eff_ws = eff_ws_raw.trim().to_string();
    if eff_ws.is_empty() && p.user_trim.contains('@') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "WORKSPACE_NOT_SET",
                message: "未设置工作区：无法在消息中使用 `@` 引用工作区内文件。请先在侧栏工作区面板选择或提交目录。"
                    .to_string(),
                reason_code: None,
            }),
        ));
    }
    let work_dir_for_expand = std::path::PathBuf::from(eff_ws_raw.clone());
    let msg = {
        let cfg = state.cfg.read().await;
        expand_at_file_refs_in_user_message(&p.user_trim, work_dir_for_expand.as_path(), &cfg)
            .map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ApiError {
                        code: "INVALID_AT_FILE_REF",
                        message: e,
                        reason_code: None,
                    }),
                )
            })?
    };
    let msg = merge_user_text_with_clarification_answers(msg, p.clarify);
    let turn_seed = build_messages_for_turn(
        &state,
        &p.conversation_id,
        &msg,
        &p.image_urls,
        p.agent_role.as_deref(),
    )
    .await
    .map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_AGENT_ROLE",
                message: e,
                reason_code: None,
            }),
        )
    })?;
    let workspace_is_set = state.workspace_is_set().await;
    let work_dir_for_job = if eff_ws.is_empty() {
        let cfg = state.cfg.read().await;
        std::path::PathBuf::from(cfg.command_exec.run_command_working_dir.clone())
    } else {
        std::path::PathBuf::from(eff_ws.clone())
    };
    let approval_session_id = match body.approval_session_id.as_deref() {
        Some(v) => Some(normalize_approval_session_id(v).ok_or((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "INVALID_APPROVAL_SESSION_ID",
                message: "approval_session_id 非法或为空".to_string(),
                reason_code: None,
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
    let request_audit = {
        let cfg = state.cfg.read().await;
        audit::web_request_audit_from_http(&cfg, &headers, peer)
    };
    if let Err(e) = state
        .chat_queue
        .try_submit_stream(chat_job_queue::StreamSubmitParams {
            job_id,
            queue_deps: state.chat_queue_job_deps.clone(),
            app: state.clone(),
            conversation_id: p.conversation_id.clone(),
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            request_agent_role: p.agent_role.clone(),
            persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
            work_dir: work_dir_for_job,
            workspace_is_set,
            temperature_override: p.temperature_override,
            seed_override: p.seed_override,
            llm_override: p.llm_override,
            executor_llm_override: p.executor_llm_override,
            execution_mode_override: p.execution_mode_override,
            request_audit,
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
                reason_code: None,
            }),
        ));
    }
    let stream = ReceiverStream::new(rx).map(|(seq, data)| sse_event_with_id(seq, data));
    let mut resp = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&p.conversation_id) {
        resp.headers_mut().insert("x-conversation-id", v);
    }
    if let Ok(v) = HeaderValue::from_str(&job_id.to_string()) {
        resp.headers_mut().insert("x-stream-job-id", v);
    }
    Ok(resp)
}
