//! Webhook 规范化、`POST /chat/async` 后台任务与任务状态查询。

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use log::{debug, error, info, warn};

use crate::chat_job_queue;
use crate::types::Message;
use crate::web::http_types::chat::{
    ApiError, ChatAsyncRequestBody, ChatAsyncSubmitResponseBody, ChatJobStatusResponseBody,
};

use super::builtin_skills::run_web_builtin_command;
use super::enqueue::parse_chat_request_for_enqueue;
use super::enqueue::{PreparedJsonChatEnqueue, prepare_json_chat_enqueue};
use crate::web::app_state::AppState;
use crate::web::audit;

fn normalize_optional_webhook_url(
    raw: Option<String>,
) -> Result<Option<reqwest::Url>, (StatusCode, Json<ApiError>)> {
    let Some(s) = raw else {
        return Ok(None);
    };
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    let u = reqwest::Url::parse(t).map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError::new("INVALID_WEBHOOK_URL", format!("{e}"))),
        )
    })?;
    if matches!(u.scheme(), "http" | "https") {
        Ok(Some(u))
    } else {
        Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_WEBHOOK_URL",
                "webhook_url 仅支持 http 或 https",
            )),
        ))
    }
}

fn normalize_webhook_secret(
    raw: Option<String>,
) -> Result<Option<String>, (StatusCode, Json<ApiError>)> {
    let Some(s) = raw else {
        return Ok(None);
    };
    let t = s.trim().to_string();
    if t.is_empty() {
        return Ok(None);
    }
    if t.len() > 256 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_WEBHOOK_SECRET",
                "webhook_secret 过长（最多 256 字符）",
            )),
        ));
    }
    Ok(Some(t))
}

async fn post_chat_job_webhook(
    client: &reqwest::Client,
    url: &reqwest::Url,
    secret: Option<&str>,
    payload: &crate::web::async_chat_job::WebhookPayload<'_>,
) {
    let mut req = client.post(url.clone()).json(payload);
    if let Some(s) = secret {
        if let Ok(v) = HeaderValue::from_str(s) {
            req = req.header("X-Crabmate-Webhook-Secret", v);
        } else {
            warn!(target: "crabmate", "webhook_secret 含非法 HTTP 头字符，跳过 X-Crabmate-Webhook-Secret");
        }
    }
    match req.timeout(std::time::Duration::from_secs(30)).send().await {
        Ok(resp) if resp.status().is_success() => {
            debug!(target: "crabmate", "async chat webhook ok status={}", resp.status());
        }
        Ok(resp) => {
            warn!(
                target: "crabmate",
                "async chat webhook non-success status={}",
                resp.status()
            );
        }
        Err(e) => {
            warn!(
                target: "crabmate",
                "async chat webhook request failed: {}",
                e
            );
        }
    }
}

async fn run_async_chat_json_job(
    state: Arc<AppState>,
    job_id: u64,
    reply_rx: tokio::sync::oneshot::Receiver<
        Result<Vec<Message>, chat_job_queue::ChatJsonJobFailure>,
    >,
    conversation_id: String,
    webhook_url: Option<reqwest::Url>,
    webhook_secret: Option<String>,
) {
    {
        let mut g = state.aux.async_chat_jobs.write().await;
        if let Some(r) = g.get_mut(&job_id) {
            r.status = crate::web::async_chat_job::ChatAsyncJobStatus::Running;
        }
    }

    let messages_res = reply_rx.await.ok();

    let (status_str, reply, revision, err_api) = match messages_res {
        Some(Ok(messages)) => {
            let reply = messages
                .last()
                .and_then(|m| crate::types::message_content_as_str(&m.content))
                .unwrap_or("")
                .to_string();
            let revision = state
                .load_conversation_seed(&conversation_id)
                .await
                .and_then(|s| s.expected_revision);
            ("completed", Some(reply), revision, None)
        }
        Some(Err(chat_job_queue::ChatJsonJobFailure::ConversationConflict)) => {
            let e = ApiError {
                code: super::super::conflict::CONVERSATION_CONFLICT_CODE,
                message: super::super::conflict::CONVERSATION_CONFLICT_MESSAGE.to_string(),
                reason_code: None,
            };
            ("failed", None, None, Some(e))
        }
        Some(Err(chat_job_queue::ChatJsonJobFailure::Agent(err))) => {
            error!(
                target: "crabmate",
                "chat async job failed job_id={} err_kind=agent_turn {}",
                job_id,
                err.diag_log_kv(),
            );
            let body = err.http_api_error();
            ("failed", None, None, Some(body))
        }
        None => (
            "failed",
            None,
            None,
            Some(ApiError::new("INTERNAL_ERROR", "对话任务被取消或内部错误")),
        ),
    };

    {
        let mut g = state.aux.async_chat_jobs.write().await;
        if let Some(r) = g.get_mut(&job_id) {
            r.status = if status_str == "completed" {
                crate::web::async_chat_job::ChatAsyncJobStatus::Completed
            } else {
                crate::web::async_chat_job::ChatAsyncJobStatus::Failed
            };
            r.reply.clone_from(&reply);
            r.conversation_revision = revision;
            r.error = err_api.clone();
        }
    }

    if let Some(ref url) = webhook_url {
        let payload = crate::web::async_chat_job::WebhookPayload {
            job_id,
            status: status_str,
            conversation_id: conversation_id.as_str(),
            conversation_revision: revision,
            reply: reply.as_deref(),
            error: err_api.as_ref(),
        };
        post_chat_job_webhook(&state.http.client, url, webhook_secret.as_deref(), &payload).await;
    }
}

pub(crate) async fn chat_async_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(body): Json<ChatAsyncRequestBody>,
) -> Result<Json<ChatAsyncSubmitResponseBody>, (StatusCode, Json<ApiError>)> {
    if body.chat.stream_resume.is_some() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "ASYNC_STREAM_RESUME_UNSUPPORTED",
                "异步任务不支持 stream_resume；请使用 POST /chat/stream",
            )),
        ));
    }
    let parsed = parse_chat_request_for_enqueue(&state, &body.chat).await?;
    if run_web_builtin_command(&state, parsed.user_trim.as_str())
        .await
        .is_some()
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "ASYNC_BUILTIN_UNSUPPORTED",
                "内置命令（如 /skills）请使用同步 POST /chat",
            )),
        ));
    }

    let webhook_url = normalize_optional_webhook_url(body.webhook_url)?;
    let webhook_secret = normalize_webhook_secret(body.webhook_secret)?;
    let conversation_id = parsed.conversation_id.clone();

    let job_id = state.chat.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();

    {
        let mut g = state.aux.async_chat_jobs.write().await;
        g.insert(
            job_id,
            crate::web::async_chat_job::ChatAsyncJobRecord {
                status: crate::web::async_chat_job::ChatAsyncJobStatus::Pending,
                conversation_id: conversation_id.clone(),
                created_at: std::time::Instant::now(),
                webhook_url: webhook_url.as_ref().map(|u| u.to_string()),
                webhook_secret: webhook_secret.clone(),
                reply: None,
                conversation_revision: None,
                error: None,
            },
        );
    }

    let PreparedJsonChatEnqueue {
        conversation_id: cid_enqueue,
        turn_seed,
        work_dir: work_dir_for_job,
        workspace_is_set,
        msg_for_log: msg,
    } = prepare_json_chat_enqueue(
        &state,
        parsed.user_trim.as_str(),
        parsed.clarify,
        &parsed.image_urls,
        parsed.agent_role.clone(),
        conversation_id.clone(),
    )
    .await?;

    debug!(
        target: "crabmate",
        "chat async 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        crate::redact::preview_chars(&msg, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat async 任务入队 job_id={}", job_id);
    let request_audit = {
        let cfg = state.http.cfg.read().await;
        audit::web_request_audit_from_http(&cfg, &headers, peer)
    };
    let submit = state
        .chat
        .chat_queue
        .try_submit_json(chat_job_queue::JsonSubmitParams {
            envelope: chat_job_queue::WebChatJobEnvelope {
                job_id,
                queue_deps: state.chat.chat_queue_job_deps.clone(),
                app: state.clone(),
                conversation_id: cid_enqueue,
                messages: turn_seed.messages,
                expected_revision: turn_seed.expected_revision,
                request_agent_role: parsed.agent_role.clone(),
                persisted_active_agent_role: turn_seed.persisted_active_agent_role.clone(),
                work_dir: work_dir_for_job,
                workspace_is_set,
                temperature_override: parsed.temperature_override,
                seed_override: parsed.seed_override,
                client_sse_protocol: parsed.client_sse_protocol,
                llm_override: parsed.llm_override.clone(),
                executor_llm_override: parsed.executor_llm_override.clone(),
                readonly_tool_ttl_cache_secs: parsed.readonly_tool_ttl_cache_secs,
                request_audit,
            },
            reply_tx,
        });

    if let Err(e) = submit {
        let mut g = state.aux.async_chat_jobs.write().await;
        g.remove(&job_id);
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

    let st = state.clone();
    let cid = conversation_id.clone();
    let wurl = webhook_url.clone();
    let wsec = webhook_secret.clone();
    tokio::spawn(async move {
        run_async_chat_json_job(st, job_id, reply_rx, cid, wurl, wsec).await;
    });

    Ok(Json(ChatAsyncSubmitResponseBody {
        job_id,
        status: "pending",
        conversation_id,
    }))
}

pub(crate) async fn chat_job_status_handler(
    State(state): State<Arc<AppState>>,
    Path(job_id): Path<u64>,
) -> Result<Json<ChatJobStatusResponseBody>, (StatusCode, Json<ApiError>)> {
    let g = state.aux.async_chat_jobs.read().await;
    let Some(rec) = g.get(&job_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ApiError::new(
                "UNKNOWN_JOB",
                "不存在或未通过 POST /chat/async 创建的任务",
            )),
        ));
    };
    let status = match rec.status {
        crate::web::async_chat_job::ChatAsyncJobStatus::Pending => "pending",
        crate::web::async_chat_job::ChatAsyncJobStatus::Running => "running",
        crate::web::async_chat_job::ChatAsyncJobStatus::Completed => "completed",
        crate::web::async_chat_job::ChatAsyncJobStatus::Failed => "failed",
    };
    Ok(Json(ChatJobStatusResponseBody {
        job_id,
        status: status.to_string(),
        conversation_id: rec.conversation_id.clone(),
        reply: rec.reply.clone(),
        conversation_revision: rec.conversation_revision,
        error: rec.error.clone(),
    }))
}
