//! CrabMate 库：DeepSeek Agent、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 `log` + `env_logger` 处理；`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。`--tui` 时不向终端写日志行，以免打乱全屏界面。

mod agent;
mod chat_job_queue;
mod config;
mod health;
mod http_client;
mod llm;
mod path_workspace;
mod redact;
mod runtime;
mod sse;
mod text_sanitize;
mod tool_registry;
mod tool_result;
mod tools;
mod types;
mod web;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::{
    Json, Router,
    extract::{Multipart, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use config::cli::{init_logging, parse_args};
use futures_util::StreamExt;
use log::{debug, error, info};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use types::{Message, messages_chat_seed};

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// `cfg` 建议使用 [`Arc`] 共享（与进程内 Web 服务状态一致），以便工具在 `spawn_blocking` 路径中复用同一份配置而不反复深拷贝。
/// 若提供 out，则流式 content 会通过 out 发送（供 SSE 等使用）；`no_stream` 为 true 时 API 使用 `stream: false`，
/// 有正文则通过 `out` 一次性下发整段。
/// 若 `render_to_terminal` 为 true，则在终端用 `markdown_to_ansi` 渲染助手回复（流式与非流式均在**整段正文到达后**输出，避免半段 Markdown）。
/// 当 `out` 为 `None` 且 `render_to_terminal` 为 true（典型 CLI）时，分阶段规划通知、分步注入 user 与各工具结果另经 `runtime::terminal_cli_transcript` 写入 stdout；通知与注入正文与 TUI 一致经 `user_message_for_chat_display`（分步长句可压缩）；助手正文与 TUI 共用 `assistant_markdown_source_for_display`。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
/// `cancel` 为 `Some` 时，各轮请求会在流式读与重试间隔中轮询其标志；置位后尽快结束并返回 `Ok`（或 `Err` 与常量 [`crate::types::LLM_CANCELLED_ERROR`] 对齐），供 TUI 等场景中止生成。
/// `per_flight` 仅 Web 队列任务传入，用于 `GET /status` 的 `per_active_jobs` 镜像；CLI/TUI 传 `None`。
#[allow(clippy::too_many_arguments)]
pub async fn run_agent_turn(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &Arc<config::AgentConfig>,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    effective_working_dir: &std::path::Path,
    workspace_is_set: bool,
    render_to_terminal: bool,
    no_stream: bool,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    per_flight: Option<std::sync::Arc<chat_job_queue::PerTurnFlight>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut loop_params = agent::agent_turn::RunLoopParams {
        client,
        api_key,
        cfg,
        tools_defs: tools,
        messages,
        out,
        effective_working_dir,
        workspace_is_set,
        no_stream,
        cancel: cancel.as_deref(),
        mode: agent::agent_turn::AgentRunMode::Web { render_to_terminal },
        per_flight,
        tui_messages_sync: None,
    };
    agent::agent_turn::run_agent_turn_common(&mut loop_params).await
}

// ---------- Web 服务 ----------

const CONVERSATION_ID_MAX_LEN: usize = 128;
const CONVERSATION_STORE_MAX_ENTRIES: usize = 512;
const CONVERSATION_STORE_TTL: Duration = Duration::from_secs(24 * 3600);

#[derive(Clone)]
struct ConversationEntry {
    messages: Vec<Message>,
    updated_at: std::time::Instant,
}

#[derive(Clone)]
pub(crate) struct AppState {
    cfg: Arc<config::AgentConfig>,
    api_key: String,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.run_command_working_dir
    workspace_override: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
    uploads_dir: std::path::PathBuf,
    /// `/chat` / `/chat/stream` 进程内任务队列（有界排队 + 并发上限）
    chat_queue: chat_job_queue::ChatJobQueue,
    /// 基于 `conversation_id` 的进程内会话存储（PR-1：内存实现；后续可替换 Redis/DB）。
    conversation_store: std::sync::Arc<tokio::sync::RwLock<HashMap<String, ConversationEntry>>>,
    /// 新会话 ID 递增计数器（仅用于生成默认 conversation_id）。
    conversation_id_counter: std::sync::Arc<AtomicU64>,
}

impl AppState {
    /// 当前生效的工作区根路径（前端已设置则用其值，否则用配置）
    async fn effective_workspace_path(&self) -> String {
        let guard = self.workspace_override.read().await;
        match guard.as_deref() {
            None => self.cfg.run_command_working_dir.clone(),
            Some(s) if s.trim().is_empty() => self.cfg.run_command_working_dir.clone(),
            Some(s) => s.to_string(),
        }
    }

    /// 前端是否已经“设置过工作区”（包含：显式选择默认目录）
    async fn workspace_is_set(&self) -> bool {
        let guard = self.workspace_override.read().await;
        guard.is_some()
    }

    fn next_conversation_id(&self) -> String {
        let n = self.conversation_id_counter.fetch_add(1, Ordering::Relaxed);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("conv_{}_{}", ts, n)
    }

    async fn load_conversation_messages(&self, conversation_id: &str) -> Option<Vec<Message>> {
        let mut guard = self.conversation_store.write().await;
        let entry = guard.get_mut(conversation_id)?;
        if entry.updated_at.elapsed() > CONVERSATION_STORE_TTL {
            guard.remove(conversation_id);
            return None;
        }
        entry.updated_at = std::time::Instant::now();
        Some(entry.messages.clone())
    }

    pub(crate) async fn save_conversation_messages(
        &self,
        conversation_id: String,
        messages: Vec<Message>,
    ) {
        let mut guard = self.conversation_store.write().await;
        let now = std::time::Instant::now();
        guard.insert(
            conversation_id,
            ConversationEntry {
                messages,
                updated_at: now,
            },
        );
        guard.retain(|_, v| now.duration_since(v.updated_at) <= CONVERSATION_STORE_TTL);
        if guard.len() > CONVERSATION_STORE_MAX_ENTRIES {
            let mut order: Vec<(String, std::time::Instant)> = guard
                .iter()
                .map(|(k, v)| (k.clone(), v.updated_at))
                .collect();
            order.sort_by_key(|(_, t)| *t);
            let to_drop = guard.len() - CONVERSATION_STORE_MAX_ENTRIES;
            for (k, _) in order.into_iter().take(to_drop) {
                guard.remove(&k);
            }
        }
    }

    async fn conversation_count(&self) -> usize {
        self.conversation_store.read().await.len()
    }
}

#[derive(serde::Deserialize)]
struct ChatRequestBody {
    message: String,
    #[serde(default)]
    conversation_id: Option<String>,
}

#[derive(serde::Serialize)]
struct UploadedFileInfo {
    url: String,
    filename: String,
    mime: String,
    size: u64,
}

#[derive(serde::Serialize)]
struct UploadResponseBody {
    files: Vec<UploadedFileInfo>,
}

#[derive(serde::Deserialize)]
struct DeleteUploadsBody {
    urls: Vec<String>,
}

#[derive(serde::Serialize)]
struct DeleteUploadsResponseBody {
    deleted: Vec<String>,
    skipped: Vec<String>,
}

async fn delete_uploads_handler(
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

async fn cleanup_uploads_dir(dir: std::path::PathBuf, max_age: Duration, max_bytes: u64) {
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

async fn upload_handler(
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
struct ChatResponseBody {
    reply: String,
    conversation_id: String,
}

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示
#[derive(serde::Serialize)]
struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    code: &'static str,
    /// 面向用户展示的友好错误信息
    message: String,
}

fn normalize_client_conversation_id(raw: Option<&str>) -> Result<Option<String>, String> {
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
) -> Vec<Message> {
    if let Some(mut messages) = state.load_conversation_messages(conversation_id).await {
        messages.push(Message::user_only(user_msg.to_string()));
        return messages;
    }
    messages_chat_seed(&state.cfg.system_prompt, user_msg)
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

async fn require_web_api_bearer_auth(
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

async fn chat_handler(
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
    let messages = build_messages_for_turn(&state, &conversation_id, msg).await;
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
            messages,
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

/// 流式 chat：返回 SSE，每个 event 的 data 为一段 content delta（或结束时一条 error JSON）
async fn chat_stream_handler(
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
    let messages = build_messages_for_turn(&state, &conversation_id, msg).await;
    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
    let workspace_is_set = state.workspace_is_set().await;
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
    if let Err(e) = state.chat_queue.try_submit_stream(
        job_id,
        state.clone(),
        conversation_id.clone(),
        messages,
        work_dir,
        workspace_is_set,
        tx,
    ) {
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

async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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
    /// 为 true 时每条用户消息先无工具规划轮再按步执行（见 `agent::agent_turn`）。
    staged_plan_execution: bool,
    /// TUI / CLI REPL 是否在启动时从 `.crabmate/tui_session.json` 恢复会话（默认 false）。
    tui_load_session_on_start: bool,
    max_message_history: usize,
    tool_message_max_chars: usize,
    context_char_budget: usize,
    context_summary_trigger_chars: usize,
    chat_queue_max_concurrent: usize,
    chat_queue_max_pending: usize,
    chat_queue_running: usize,
    chat_queue_completed_ok: u64,
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

async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
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
        chat_queue_completed_err: state.chat_queue.completed_err(),
        chat_queue_recent_jobs: state.chat_queue.recent_jobs(),
        per_active_jobs: state.chat_queue.active_per_jobs(),
        workspace_allowed_roots_count: state.cfg.workspace_allowed_roots.len(),
        conversation_store_entries,
    })
}

/// CLI 入口逻辑（与历史二进制 `main` 等价）：解析参数、加载配置、启动 Web / REPL / TUI。
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let (
        config_path,
        single_shot,
        serve_port,
        http_bind_host,
        workspace_cli,
        output_mode,
        no_tools,
        no_web,
        dry_run,
        no_stream,
        tui,
        log_file,
    ) = parse_args();

    // 非 Web `--serve` 的 CLI 默认不输出 info（仅 warn+），除非设置 RUST_LOG 或 `--log` 文件（见 `init_logging`）
    init_logging(
        tui,
        log_file.as_deref().map(std::path::Path::new),
        serve_port.is_none(),
    );

    let api_key = match env::var("API_KEY") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("请设置环境变量 API_KEY");
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "未设置环境变量 API_KEY",
            )
            .into());
        }
    };

    let cfg = match config::load_config(config_path.as_deref()) {
        Ok(c) => Arc::new(c),
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    info!(
        target: "crabmate",
        "配置已加载 api_base={} model={}",
        cfg.api_base,
        cfg.model
    );
    if dry_run {
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        if !static_dir.is_dir() {
            eprintln!(
                "dry-run 失败：前端静态目录不存在：{}（请先在 frontend/ 下构建）",
                static_dir.display()
            );
            std::process::exit(1);
        }
        println!(
            "配置检查通过：API_KEY 已设置，配置可用，前端静态目录存在：{}",
            static_dir.display()
        );
        return Ok(());
    }
    let client = http_client::build_shared_api_client(cfg.as_ref())?;
    let all_tools = tools::build_tools();
    let tools = if no_tools { Vec::new() } else { all_tools };

    if tui {
        crate::runtime::tui::run_tui(&cfg, &client, &api_key, &tools, &workspace_cli, no_stream)
            .await?;
        return Ok(());
    }

    if let Some(port) = serve_port {
        let initial_workspace = workspace_cli.clone();
        let uploads_dir = std::env::temp_dir().join("crabmate_uploads");
        std::fs::create_dir_all(&uploads_dir).ok();
        let chat_queue = chat_job_queue::ChatJobQueue::new(
            cfg.chat_queue_max_concurrent,
            cfg.chat_queue_max_pending,
        );
        let state = Arc::new(AppState {
            cfg: Arc::clone(&cfg),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
            uploads_dir: uploads_dir.clone(),
            chat_queue,
            conversation_store: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            conversation_id_counter: std::sync::Arc::new(AtomicU64::new(1)),
        });
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        let uploads_dir_for_static = uploads_dir.clone();
        let mut protected_api = Router::new()
            .route("/chat", post(chat_handler))
            .route("/chat/stream", post(chat_stream_handler))
            .route("/upload", post(upload_handler))
            .route("/uploads/delete", post(delete_uploads_handler))
            .route(
                "/workspace",
                get(web::workspace::workspace_handler).post(web::workspace::workspace_set_handler),
            )
            .route(
                "/workspace/pick",
                get(web::workspace::workspace_pick_handler),
            )
            .route(
                "/workspace/search",
                post(web::workspace::workspace_search_handler),
            )
            .route(
                "/workspace/file",
                get(web::workspace::workspace_file_read_handler)
                    .post(web::workspace::workspace_file_write_handler)
                    .delete(web::workspace::workspace_file_delete_handler),
            )
            .route(
                "/tasks",
                get(web::task::tasks_get_handler).post(web::task::tasks_set_handler),
            );
        if !state.cfg.web_api_bearer_token.trim().is_empty() {
            protected_api = protected_api.route_layer(middleware::from_fn_with_state(
                state.clone(),
                require_web_api_bearer_auth,
            ));
        }
        let mut app = Router::new()
            .merge(protected_api)
            .route("/health", get(health_handler))
            .route("/status", get(status_handler))
            .nest_service(
                "/uploads",
                ServiceBuilder::new()
                    .layer(SetResponseHeaderLayer::if_not_present(
                        header::CACHE_CONTROL,
                        HeaderValue::from_static("public, max-age=31536000, immutable"),
                    ))
                    .layer(SetResponseHeaderLayer::if_not_present(
                        header::X_CONTENT_TYPE_OPTIONS,
                        HeaderValue::from_static("nosniff"),
                    ))
                    .layer(SetResponseHeaderLayer::if_not_present(
                        header::HeaderName::from_static("cross-origin-resource-policy"),
                        HeaderValue::from_static("same-site"),
                    ))
                    .service(ServeDir::new(uploads_dir_for_static)),
            );
        if !no_web {
            app = app.nest_service("/", ServeDir::new(static_dir));
        }
        let app = app.with_state(state);
        let bind_ip: std::net::IpAddr = http_bind_host.parse().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "无效的 Web 监听地址 {:?}（请使用有效 IP，如 127.0.0.1 或 0.0.0.0）",
                    http_bind_host
                ),
            )
        })?;
        let auth_enabled = !cfg.web_api_bearer_token.trim().is_empty();
        if !auth_enabled && !cfg.allow_insecure_no_auth_for_non_loopback {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "未配置 web_api_bearer_token，默认拒绝无鉴权启动；请设置 [agent].web_api_bearer_token / AGENT_WEB_API_BEARER_TOKEN，或显式设置 allow_insecure_no_auth_for_non_loopback=true（不安全）",
            )
            .into());
        }
        let addr = std::net::SocketAddr::from((bind_ip, port));
        println!("Web 服务已启动");
        println!("  监听: http://{}/", addr);
        if !auth_enabled {
            eprintln!(
                "  警告: 当前按 allow_insecure_no_auth_for_non_loopback=true 无鉴权运行（{}），请勿在不可信网络暴露",
                addr
            );
        }
        if auth_enabled {
            println!("  安全: 已启用 Bearer 鉴权（Authorization 头）");
        }
        info!(target: "crabmate", "Web 服务监听 addr={}", addr);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        // uploads 自动清理：每 10 分钟执行一次；保留 24h；总容量上限 500MB
        tokio::spawn({
            let dir = uploads_dir.clone();
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(600));
                loop {
                    interval.tick().await;
                    cleanup_uploads_dir(
                        dir.clone(),
                        Duration::from_secs(24 * 3600),
                        500 * 1024 * 1024,
                    )
                    .await;
                }
            }
        });
        axum::serve(listener, app).await?;
        return Ok(());
    }

    if let Some(question) = single_shot {
        crate::runtime::cli::run_single_shot(
            &cfg,
            &client,
            &api_key,
            &tools,
            &workspace_cli,
            &output_mode,
            no_stream,
            question,
        )
        .await?;
        return Ok(());
    }

    crate::runtime::cli::run_repl(&cfg, &client, &api_key, &tools, &workspace_cli, no_stream).await
}

pub use config::{AgentConfig, load_config};
pub use tool_registry::{
    ToolDispatchMeta, ToolExecutionClass, all_dispatch_metadata, execution_class_for_tool,
    try_dispatch_meta,
};
pub use tools::dev_tag;
pub use tools::{ToolsBuildOptions, build_tools, build_tools_filtered, build_tools_with_options};

#[cfg(test)]
mod tests {
    use super::normalize_client_conversation_id;

    #[test]
    fn normalize_conversation_id_accepts_expected_charset() {
        let got = normalize_client_conversation_id(Some("conv_abc-123:xyz.test"))
            .expect("id should parse");
        assert_eq!(got.as_deref(), Some("conv_abc-123:xyz.test"));
    }

    #[test]
    fn normalize_conversation_id_rejects_invalid_chars() {
        let err = normalize_client_conversation_id(Some("abc/def")).expect_err("should reject /");
        assert!(err.contains("仅允许"));
    }
}
