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

use super::app_state::{AppState, CONVERSATION_ID_MAX_LEN, ConversationTurnSeed};
use crate::agent_memory::{load_memory_snippet, messages_chat_seed_with_memory};
use crate::chat_job_queue;
use crate::conversation_store::SaveConversationOutcome;
use crate::health;
use crate::project_profile::{build_project_profile_markdown, merge_memory_and_profile_snippets};
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
    /// 覆盖本回合 `chat/completions` 的 **`temperature`**（0～2）；省略则用服务端配置。
    #[serde(default)]
    temperature: Option<f64>,
    /// 写入请求 JSON 的整数 **`seed`**（OpenAI 兼容）；与 `seed_policy: "omit"` 互斥。
    #[serde(default)]
    seed: Option<i64>,
    /// `omit` / `none`：本回合请求**不**带 `seed`（即使配置了默认 `llm_seed`）。
    #[serde(default)]
    seed_policy: Option<String>,
}

fn parse_optional_chat_temperature(raw: Option<f64>) -> Result<Option<f32>, String> {
    let Some(t) = raw else {
        return Ok(None);
    };
    if !t.is_finite() {
        return Err("temperature 须为有限浮点数".to_string());
    }
    let t = t as f32;
    if !(0.0..=2.0).contains(&t) {
        return Err("temperature 须在 0～2 之间".to_string());
    }
    Ok(Some(t))
}

fn parse_seed_override_from_body(
    seed: Option<i64>,
    seed_policy: Option<String>,
) -> Result<crate::LlmSeedOverride, String> {
    let policy = seed_policy
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match (seed, policy) {
        (Some(_), Some(p)) if p.eq_ignore_ascii_case("omit") || p.eq_ignore_ascii_case("none") => {
            Err("seed 与 seed_policy=omit 不能同时使用".to_string())
        }
        (Some(n), _) => Ok(crate::LlmSeedOverride::Fixed(n)),
        (None, Some(p)) if p.eq_ignore_ascii_case("omit") || p.eq_ignore_ascii_case("none") => {
            Ok(crate::LlmSeedOverride::OmitFromRequest)
        }
        (None, Some(p)) => Err(format!(
            "未知的 seed_policy: {:?}（支持 omit、none 或省略）",
            p
        )),
        (None, None) => Ok(crate::LlmSeedOverride::FromConfig),
    }
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

/// Web：将会话在服务端截断到第 `before_user_ordinal` 条**普通**用户消息之前（0-based，与前端用户气泡序号一致）。
#[derive(serde::Deserialize)]
pub(crate) struct ChatBranchRequestBody {
    conversation_id: String,
    /// 从此序号对应的用户消息起（含）全部丢弃；例如 `1` 表示保留第 0 条用户及之前上下文。
    before_user_ordinal: u64,
    /// 截断前客户端所知的 `revision`（与冲突检测一致；可从最近一次成功回合推断）。
    expected_revision: u64,
}

#[derive(serde::Serialize)]
pub(crate) struct ChatBranchResponseBody {
    ok: bool,
    /// 截断成功后的 revision（与 `keep_message_count == 当前长度` 时也会递增一次的行为一致：仅当 SQLite/内存实际执行了 UPDATE）。
    revision: u64,
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
    /// 写入存储后的 revision（供 `POST /chat/branch`）；无持久化会话时可能为 null。
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_revision: Option<u64>,
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
    let root = std::path::PathBuf::from(state.effective_workspace_path().await);
    let memory_snippet = if state.cfg.agent_memory_file_enabled {
        load_memory_snippet(
            &root,
            state.cfg.agent_memory_file.as_str(),
            state.cfg.agent_memory_file_max_chars,
        )
    } else {
        None
    };

    let messages = if !state.cfg.project_profile_inject_enabled {
        messages_chat_seed_with_memory(
            &state.cfg.system_prompt,
            user_msg,
            memory_snippet.as_deref(),
        )
    } else {
        let max_chars = state.cfg.project_profile_inject_max_chars;
        let root_for_profile = root.clone();
        let profile_md = match tokio::task::spawn_blocking(move || {
            build_project_profile_markdown(&root_for_profile, max_chars)
        })
        .await
        {
            Ok(s) => s,
            Err(e) => {
                debug!("project_profile spawn_blocking failed: {}", e);
                String::new()
            }
        };
        let combined =
            merge_memory_and_profile_snippets(memory_snippet.as_deref(), profile_md.as_str());
        match combined {
            Some(ctx) => vec![
                Message::system_only(state.cfg.system_prompt.clone()),
                Message::user_only(ctx),
                Message::user_only(user_msg.to_string()),
            ],
            None => messages_chat_seed(&state.cfg.system_prompt, user_msg),
        }
    };
    ConversationTurnSeed {
        messages,
        expected_revision: None,
    }
}

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
    }))
}

fn conversation_conflict_api_error() -> (StatusCode, Json<ApiError>) {
    conversation_conflict_http_response()
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
        .try_submit_json(chat_job_queue::JsonSubmitParams {
            job_id,
            state: state.clone(),
            conversation_id: conversation_id.clone(),
            messages: turn_seed.messages,
            expected_revision: turn_seed.expected_revision,
            work_dir: std::path::PathBuf::from(work_dir),
            workspace_is_set,
            temperature_override,
            seed_override,
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
        .and_then(|m| m.content.as_deref())
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
            temperature_override,
            seed_override,
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
    /// 默认写入 `chat/completions` 的整数 seed（未配置则为 `null`）。
    llm_seed: Option<i64>,
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
    parallel_readonly_tools_max: usize,
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
    /// 长期记忆是否启用（配置）。
    long_term_memory_enabled: bool,
    /// 向量后端：`disabled` / `fastembed` 等。
    long_term_memory_vector_backend: String,
    /// 本进程是否已挂载记忆运行时（含与会话库共用 SQLite 或独立库路径）。
    long_term_memory_store_ready: bool,
    /// 异步索引累计失败次数（成功回合不递增；仅排障用）。
    long_term_memory_index_errors: u64,
    /// Web 新会话首轮是否注入自动生成的项目画像 Markdown。
    project_profile_inject_enabled: bool,
    /// 项目画像注入正文最大字符数（0 表示关闭生成）。
    project_profile_inject_max_chars: usize,
    /// 是否要求非只读工具在 JSON 中带 `crabmate_explain_why`。
    tool_call_explain_enabled: bool,
    tool_call_explain_min_chars: usize,
    tool_call_explain_max_chars: usize,
}

pub(crate) async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let conversation_store_entries = state.conversation_count().await;
    let (ltm_ready, ltm_idx_err) = match state.long_term_memory.as_ref() {
        Some(l) => (
            true,
            l.index_errors.load(std::sync::atomic::Ordering::Relaxed),
        ),
        None => (false, 0u64),
    };
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
        llm_seed: state.cfg.llm_seed,
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
        parallel_readonly_tools_max: state.cfg.parallel_readonly_tools_max,
        chat_queue_running: state.chat_queue.running_count(),
        chat_queue_completed_ok: state.chat_queue.completed_ok(),
        chat_queue_completed_cancelled: state.chat_queue.completed_cancelled(),
        chat_queue_completed_err: state.chat_queue.completed_err(),
        chat_queue_recent_jobs: state.chat_queue.recent_jobs(),
        per_active_jobs: state.chat_queue.active_per_jobs(),
        workspace_allowed_roots_count: state.cfg.workspace_allowed_roots.len(),
        conversation_store_entries,
        long_term_memory_enabled: state.cfg.long_term_memory_enabled,
        long_term_memory_vector_backend: state
            .cfg
            .long_term_memory_vector_backend
            .as_str()
            .to_string(),
        long_term_memory_store_ready: ltm_ready,
        long_term_memory_index_errors: ltm_idx_err,
        project_profile_inject_enabled: state.cfg.project_profile_inject_enabled,
        project_profile_inject_max_chars: state.cfg.project_profile_inject_max_chars,
        tool_call_explain_enabled: state.cfg.tool_call_explain_enabled,
        tool_call_explain_min_chars: state.cfg.tool_call_explain_min_chars,
        tool_call_explain_max_chars: state.cfg.tool_call_explain_max_chars,
    })
}
