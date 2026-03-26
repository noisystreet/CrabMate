//! `/chat`、`/upload`、`/health`、`/status` 等 Axum handler（自 `lib.rs` 下沉）。
//!
//! 本模块为 `web` 私有子模块；路由在 `server.rs` 中通过 `super::chat_handlers` 引用。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Multipart, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use log::{debug, error, info};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::app_state::{
    AppState, CONVERSATION_ID_MAX_LEN, ConversationTurnSeed, SaveConversationOutcome,
};
use crate::chat_job_queue;
use crate::health;
use crate::redact;
use crate::tool_registry;
use crate::types::{CommandApprovalDecision, Message, messages_chat_seed};

#[derive(serde::Deserialize)]
pub(crate) struct ChatRequestBody {
    message: String,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    approval_session_id: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ChatApprovalRequestBody {
    approval_session_id: String,
    decision: String,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatApprovalResponseBody {
    ok: bool,
}

fn normalize_approval_session_id(raw: &str) -> Option<String> {
    let s = raw.trim();
    if s.is_empty() || s.len() > 128 {
        return None;
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return None;
    }
    Some(s.to_string())
}

#[derive(serde::Serialize)]
struct UploadedFileInfo {
    url: String,
    filename: String,
    mime: String,
    size: u64,
}

#[derive(serde::Serialize)]
pub(crate) struct UploadResponseBody {
    files: Vec<UploadedFileInfo>,
}

#[derive(serde::Deserialize)]
pub(crate) struct DeleteUploadsBody {
    urls: Vec<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct DeleteUploadsResponseBody {
    deleted: Vec<String>,
    skipped: Vec<String>,
}

pub(crate) async fn delete_uploads_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DeleteUploadsBody>,
) -> Result<Json<DeleteUploadsResponseBody>, (StatusCode, Json<ApiError>)> {
    let mut deleted = Vec::new();
    let mut skipped = Vec::new();
    for u in body.urls {
        // 只接受 /uploads/<filename> 形式，避免目录穿越
        if !u.starts_with("/uploads/") || u.contains("..") || u.contains('\\') {
            skipped.push(u);
            continue;
        }
        let name = u.trim_start_matches("/uploads/");
        if name.is_empty() || name.contains('/') {
            skipped.push(u);
            continue;
        }
        let path = state.uploads_dir.join(name);
        // 不暴露更多信息：不存在也当作 skipped
        match tokio::fs::remove_file(&path).await {
            Ok(()) => deleted.push(format!("/uploads/{}", name)),
            Err(_) => skipped.push(format!("/uploads/{}", name)),
        }
    }
    Ok(Json(DeleteUploadsResponseBody { deleted, skipped }))
}

pub(crate) async fn cleanup_uploads_dir(
    dir: std::path::PathBuf,
    max_age: std::time::Duration,
    max_bytes: u64,
) {
    let now = std::time::SystemTime::now();
    let mut entries: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = Vec::new();
    let mut total: u64 = 0;

    let mut rd = match tokio::fs::read_dir(&dir).await {
        Ok(r) => r,
        Err(e) => {
            error!(
                target: "crabmate",
                "uploads 清理：无法读取目录 dir={} error={}",
                dir.display(),
                e
            );
            return;
        }
    };

    while let Ok(Some(ent)) = rd.next_entry().await {
        let path = ent.path();
        let meta = match ent.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        if !meta.is_file() {
            continue;
        }
        let size = meta.len();
        let mtime = meta.modified().unwrap_or(now);
        total = total.saturating_add(size);
        entries.push((path, mtime, size));
    }

    // 1) 先按时间清理
    let mut kept: Vec<(std::path::PathBuf, std::time::SystemTime, u64)> = Vec::new();
    for (p, mt, sz) in entries {
        let too_old = now
            .duration_since(mt)
            .ok()
            .map(|d| d > max_age)
            .unwrap_or(false);
        if too_old {
            if tokio::fs::remove_file(&p).await.is_ok() {
                total = total.saturating_sub(sz);
            }
        } else {
            kept.push((p, mt, sz));
        }
    }

    // 2) 再按容量清理（从最旧开始删，直到 <= max_bytes）
    if total > max_bytes {
        kept.sort_by_key(|x| x.1);
        for (p, _mt, sz) in kept {
            if total <= max_bytes {
                break;
            }
            if tokio::fs::remove_file(&p).await.is_ok() {
                total = total.saturating_sub(sz);
            }
        }
    }
}

fn sanitize_display_filename(input: &str) -> String {
    // 仅用于“展示给前端”，不参与落盘路径（落盘用服务端生成的 safe_name）
    let base = std::path::Path::new(input)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("upload.bin");
    let mut out = String::with_capacity(base.len().min(80));
    for ch in base.chars() {
        let ok = ch.is_ascii_alphanumeric()
            || matches!(ch, '.' | '-' | '_' | ' ' | '(' | ')' | '[' | ']');
        out.push(if ok { ch } else { '_' });
        if out.len() >= 80 {
            break;
        }
    }
    if out.trim().is_empty() {
        "upload.bin".to_string()
    } else {
        out
    }
}

fn ext_lower(file_name: &str) -> Option<String> {
    std::path::Path::new(file_name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
}

pub(crate) async fn upload_handler(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponseBody>, (StatusCode, Json<ApiError>)> {
    let mut out: Vec<UploadedFileInfo> = Vec::new();
    let max_total: u64 = 200 * 1024 * 1024; // 200MB total
    let max_files: usize = 20;
    let mut total: u64 = 0;

    static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

    while let Some(field) = multipart.next_field().await.map_err(|e| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "MULTIPART_ERROR",
                message: format!("上传解析失败：{}", e),
            }),
        )
    })? {
        if out.len() >= max_files {
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                Json(ApiError {
                    code: "UPLOAD_TOO_MANY_FILES",
                    message: "上传文件数量过多".to_string(),
                }),
            ));
        }

        let raw_name = field.file_name().unwrap_or("upload.bin");
        let file_name = sanitize_display_filename(raw_name);
        let mime = field
            .content_type()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        // 白名单：MIME 前缀 + 扩展名
        let ext = ext_lower(&file_name).unwrap_or_default();
        let is_image = mime.starts_with("image/")
            && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif");
        let is_audio = mime.starts_with("audio/")
            && matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "aac" | "ogg" | "webm");
        let is_video =
            mime.starts_with("video/") && matches!(ext.as_str(), "mp4" | "webm" | "mov" | "mkv");
        if !(is_image || is_audio || is_video) {
            return Err((
                StatusCode::UNSUPPORTED_MEDIA_TYPE,
                Json(ApiError {
                    code: "UPLOAD_UNSUPPORTED_TYPE",
                    message: "不支持的文件类型（仅支持常见图片/音频/视频）".to_string(),
                }),
            ));
        }

        // 单文件大小限制（与前端保持同量级）
        let max_single: u64 = if is_image {
            8 * 1024 * 1024
        } else if is_audio {
            25 * 1024 * 1024
        } else {
            80 * 1024 * 1024
        };

        let ext_with_dot = if ext.is_empty() {
            "".to_string()
        } else {
            format!(".{}", ext)
        };

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let safe_name = format!("u{}_{}_{}{}", std::process::id(), ts, n, ext_with_dot);
        let path = state.uploads_dir.join(&safe_name);

        let mut f = tokio::fs::File::create(&path).await.map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ApiError {
                    code: "UPLOAD_WRITE_ERROR",
                    message: format!("无法写入上传文件：{}", e),
                }),
            )
        })?;

        let mut size: u64 = 0;
        let mut field = field;
        loop {
            let next = match field.chunk().await {
                Ok(v) => v,
                Err(e) => {
                    let _ = tokio::fs::remove_file(&path).await;
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(ApiError {
                            code: "UPLOAD_READ_ERROR",
                            message: format!("读取上传内容失败：{}", e),
                        }),
                    ));
                }
            };
            let Some(chunk) = next else {
                break;
            };
            let chunk_len = chunk.len() as u64;
            size += chunk_len;
            total += chunk_len;
            if size > max_single {
                let _ = tokio::fs::remove_file(&path).await;
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(ApiError {
                        code: "UPLOAD_FILE_TOO_LARGE",
                        message: "单个文件过大".to_string(),
                    }),
                ));
            }
            if total > max_total {
                let _ = tokio::fs::remove_file(&path).await;
                return Err((
                    StatusCode::PAYLOAD_TOO_LARGE,
                    Json(ApiError {
                        code: "UPLOAD_TOO_LARGE",
                        message: "上传内容过大".to_string(),
                    }),
                ));
            }
            use tokio::io::AsyncWriteExt;
            f.write_all(&chunk).await.map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ApiError {
                        code: "UPLOAD_WRITE_ERROR",
                        message: format!("写入上传内容失败：{}", e),
                    }),
                )
            })?;
        }

        let url = format!("/uploads/{}", safe_name);
        out.push(UploadedFileInfo {
            url,
            filename: file_name,
            mime,
            size,
        });
    }

    Ok(Json(UploadResponseBody { files: out }))
}

#[derive(serde::Serialize)]
pub(crate) struct ChatResponseBody {
    reply: String,
    conversation_id: String,
}

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示
#[derive(serde::Serialize)]
pub(crate) struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    pub code: &'static str,
    /// 面向用户展示的友好错误信息
    pub message: String,
}

pub(crate) fn normalize_client_conversation_id(
    raw: Option<&str>,
) -> Result<Option<String>, String> {
    let Some(id) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if id.len() > CONVERSATION_ID_MAX_LEN {
        return Err(format!(
            "conversation_id 过长（最多 {} 个字符）",
            CONVERSATION_ID_MAX_LEN
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("conversation_id 仅允许字母、数字、- _ . :".to_string());
    }
    Ok(Some(id.to_string()))
}

async fn build_messages_for_turn(
    state: &Arc<AppState>,
    conversation_id: &str,
    user_msg: &str,
) -> ConversationTurnSeed {
    if let Some(mut seed) = state.load_conversation_seed(conversation_id).await {
        seed.messages.push(Message::user_only(user_msg.to_string()));
        return seed;
    }
    ConversationTurnSeed {
        messages: messages_chat_seed(&state.cfg.system_prompt, user_msg),
        expected_revision: None,
    }
}

fn conversation_conflict_api_error() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::CONFLICT,
        Json(ApiError {
            code: "CONVERSATION_CONFLICT",
            message: "会话已被其他请求更新，请重试本次提问".to_string(),
        }),
    )
}

pub(crate) fn save_outcome_to_stream_error_line(
    outcome: SaveConversationOutcome,
) -> Option<String> {
    match outcome {
        SaveConversationOutcome::Saved => None,
        SaveConversationOutcome::Conflict => Some(crate::sse::encode_message(
            crate::sse::SsePayload::Error(crate::sse::SseErrorBody {
                error: "会话已被其他请求更新，请重试本次提问".to_string(),
                code: Some("CONVERSATION_CONFLICT".to_string()),
            }),
        )),
    }
}

fn is_valid_bearer_header(
    auth_header: Option<&axum::http::header::HeaderValue>,
    token: &str,
) -> bool {
    if token.is_empty() {
        return true;
    }
    let Some(raw) = auth_header else {
        return false;
    };
    let Ok(v) = raw.to_str() else {
        return false;
    };
    let expected = format!("Bearer {}", token);
    v.trim() == expected
}

pub(crate) async fn require_web_api_bearer_auth(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let token = state.cfg.web_api_bearer_token.trim();
    if token.is_empty() {
        return next.run(req).await;
    }
    if is_valid_bearer_header(req.headers().get(header::AUTHORIZATION), token) {
        return next.run(req).await;
    }
    (
        StatusCode::UNAUTHORIZED,
        Json(ApiError {
            code: "UNAUTHORIZED",
            message: "缺少或无效的 Authorization Bearer token".to_string(),
        }),
    )
        .into_response()
}

pub(crate) async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Json<ChatResponseBody>, (StatusCode, Json<ApiError>)> {
    let msg = body.message.trim();
    if msg.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空".to_string(),
            }),
        ));
    }
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
    let turn_seed = build_messages_for_turn(&state, &conversation_id, msg).await;
    let work_dir_str = state.effective_workspace_path().await;
    let work_dir = work_dir_str.clone();
    let workspace_is_set = state.workspace_is_set().await;
    let job_id = state.chat_queue.next_job_id();
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    debug!(
        target: "crabmate",
        "chat json 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    info!(target: "crabmate", "chat json 任务入队 job_id={}", job_id);
    state
        .chat_queue
        .try_submit_json(
            job_id,
            state.clone(),
            conversation_id.clone(),
            turn_seed.messages,
            turn_seed.expected_revision,
            std::path::PathBuf::from(work_dir),
            workspace_is_set,
            reply_tx,
        )
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
        .and_then(|m| m.content.as_deref())
        .unwrap_or("")
        .to_string();
    Ok(Json(ChatResponseBody {
        reply,
        conversation_id,
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

/// 流式 chat：返回 SSE，每个 event 的 data 为一段 content delta（或结束时一条 error JSON）
pub(crate) async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Response, (StatusCode, Json<ApiError>)> {
    let msg = body.message.trim();
    if msg.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "EMPTY_MESSAGE",
                message: "提问内容不能为空".to_string(),
            }),
        ));
    }
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
    let turn_seed = build_messages_for_turn(&state, &conversation_id, msg).await;
    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
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
    let (tx, rx) = mpsc::channel::<String>(1024);
    debug!(
        target: "crabmate",
        "chat stream 请求摘要 job_id={} user_len={} user_preview={}",
        job_id,
        msg.len(),
        redact::preview_chars(msg, redact::MESSAGE_LOG_PREVIEW_CHARS)
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
            work_dir,
            workspace_is_set,
            sse_tx: tx,
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
    let stream = ReceiverStream::new(rx)
        .map(|s| Ok::<Event, std::convert::Infallible>(Event::default().data(s)));
    let mut resp = Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(v) = HeaderValue::from_str(&conversation_id) {
        resp.headers_mut().insert("x-conversation-id", v);
    }
    Ok(resp)
}

pub(crate) async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
    let report = health::build_health_report(&work_dir, &state.api_key, true).await;
    Json(report)
}

#[derive(serde::Serialize)]
struct StatusResponse {
    status: &'static str,
    model: String,
    api_base: String,
    max_tokens: u32,
    temperature: f32,
    /// 当前加载进 API 请求的工具定义数量（`--no-tools` 时为 0）。
    tool_count: usize,
    /// 与模型对话时实际下发的工具名列表。
    tool_names: Vec<String>,
    /// `tool_registry` 中显式声明的分发策略（其余名称运行时走同步 `run_tool`）。
    tool_dispatch_registry: &'static [tool_registry::ToolDispatchMeta],
    reflection_default_max_rounds: usize,
    final_plan_requirement: crate::agent::per_coord::FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    /// 规划器/执行器模式：single_agent | logical_dual_agent。
    planner_executor_mode: &'static str,
    /// 为 true 时每条用户消息先无工具规划轮再按步执行（见 `agent::agent_turn`）。
    staged_plan_execution: bool,
    /// CLI REPL 是否在启动时从 `.crabmate/tui_session.json` 恢复会话（默认 false；文件名历史兼容）。
    tui_load_session_on_start: bool,
    max_message_history: usize,
    tool_message_max_chars: usize,
    context_char_budget: usize,
    context_summary_trigger_chars: usize,
    chat_queue_max_concurrent: usize,
    chat_queue_max_pending: usize,
    chat_queue_running: usize,
    chat_queue_completed_ok: u64,
    chat_queue_completed_cancelled: u64,
    chat_queue_completed_err: u64,
    chat_queue_recent_jobs: Vec<chat_job_queue::ChatJobRecord>,
    /// 队列中正在执行的 `/chat`、`/chat/stream` 任务之 PER 镜像（无任务或无非队列调用时为空）。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    per_active_jobs: Vec<chat_job_queue::PerFlightStatusEntry>,
    /// Web `POST /workspace` 允许的工作区根目录个数（未配置 `workspace_allowed_roots` 时为 1，即仅 `run_command_working_dir`）。
    workspace_allowed_roots_count: usize,
    /// 当前内存会话存储中的会话数量（按 `conversation_id`）。
    conversation_store_entries: usize,
}

pub(crate) async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let conversation_store_entries = state.conversation_count().await;
    let tool_names: Vec<String> = state
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    Json(StatusResponse {
        status: "ok",
        model: state.cfg.model.clone(),
        api_base: state.cfg.api_base.clone(),
        max_tokens: state.cfg.max_tokens,
        temperature: state.cfg.temperature,
        tool_count: tool_names.len(),
        tool_names,
        tool_dispatch_registry: tool_registry::all_dispatch_metadata(),
        reflection_default_max_rounds: state.cfg.reflection_default_max_rounds,
        final_plan_requirement: state.cfg.final_plan_requirement,
        plan_rewrite_max_attempts: state.cfg.plan_rewrite_max_attempts,
        planner_executor_mode: state.cfg.planner_executor_mode.as_str(),
        staged_plan_execution: state.cfg.staged_plan_execution,
        tui_load_session_on_start: state.cfg.tui_load_session_on_start,
        max_message_history: state.cfg.max_message_history,
        tool_message_max_chars: state.cfg.tool_message_max_chars,
        context_char_budget: state.cfg.context_char_budget,
        context_summary_trigger_chars: state.cfg.context_summary_trigger_chars,
        chat_queue_max_concurrent: state.chat_queue.max_concurrent(),
        chat_queue_max_pending: state.chat_queue.max_pending(),
        chat_queue_running: state.chat_queue.running_count(),
        chat_queue_completed_ok: state.chat_queue.completed_ok(),
        chat_queue_completed_cancelled: state.chat_queue.completed_cancelled(),
        chat_queue_completed_err: state.chat_queue.completed_err(),
        chat_queue_recent_jobs: state.chat_queue.recent_jobs(),
        per_active_jobs: state.chat_queue.active_per_jobs(),
        workspace_allowed_roots_count: state.cfg.workspace_allowed_roots.len(),
        conversation_store_entries,
    })
}
