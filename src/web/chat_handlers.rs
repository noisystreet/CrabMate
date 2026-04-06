//! `/chat`、`/upload`、`/health`、`/status` 等 Axum handler（自 `lib.rs` 下沉）。
//!
//! 本模块为 `web` 私有子模块；路由在 `server.rs` 中通过 `super::chat_handlers` 引用。

use std::sync::Arc;

use axum::Json;
use axum::extract::{Multipart, Query, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::Next;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use log::{debug, error, info};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use super::app_state::{AppState, CONVERSATION_ID_MAX_LEN, ConversationTurnSeed};
use crate::agent::message_pipeline::MESSAGE_PIPELINE_COUNTERS;
use crate::agent_memory::load_memory_snippet;
use crate::chat_job_queue;
use crate::config::{ExposeSecret, LlmHttpAuthMode};
use crate::conversation_store::SaveConversationOutcome;
use crate::health;
use crate::project_profile::build_first_turn_user_context_markdown;
use crate::redact;
use crate::tool_registry;
use crate::types::{CommandApprovalDecision, Message, messages_chat_seed};
use crate::workspace_changelist;

#[derive(serde::Deserialize)]
pub(crate) struct ChatRequestBody {
    message: String,
    #[serde(default)]
    conversation_id: Option<String>,
    /// 新建会话（无 `conversation_id` 或服务端尚无该 id）时选用命名角色；须与配置中角色 id 一致。已有会话时忽略。
    #[serde(default, rename = "agent_role")]
    agent_role: Option<String>,
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
    /// 可选：浏览器侧覆盖本回合 LLM 网关 `api_base` / `model` / `api_key`（不写服务端配置）。
    #[serde(default)]
    client_llm: Option<ClientLlmBody>,
}

/// `ChatRequestBody::client_llm` 的 JSON 形状（与前端 `client_llm` 对象一致）。
#[derive(serde::Deserialize, Default)]
struct ClientLlmBody {
    #[serde(default)]
    api_base: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    api_key: Option<String>,
}

const CLIENT_LLM_API_BASE_MAX: usize = 2048;
const CLIENT_LLM_MODEL_MAX: usize = 512;
const CLIENT_LLM_API_KEY_MAX: usize = 16384;

fn parse_client_llm_override(
    raw: Option<ClientLlmBody>,
) -> Result<Option<chat_job_queue::WebChatLlmOverride>, String> {
    let Some(b) = raw else {
        return Ok(None);
    };
    let api_base = b
        .api_base
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let model = b
        .model
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let api_key = b
        .api_key
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    if api_base.is_none() && model.is_none() && api_key.is_none() {
        return Ok(None);
    }
    if let Some(ref s) = api_base
        && s.len() > CLIENT_LLM_API_BASE_MAX
    {
        return Err(format!(
            "client_llm.api_base 过长（上限 {} 字符）",
            CLIENT_LLM_API_BASE_MAX
        ));
    }
    if let Some(ref s) = model
        && s.len() > CLIENT_LLM_MODEL_MAX
    {
        return Err(format!(
            "client_llm.model 过长（上限 {} 字符）",
            CLIENT_LLM_MODEL_MAX
        ));
    }
    if let Some(ref s) = api_key
        && s.len() > CLIENT_LLM_API_KEY_MAX
    {
        return Err(format!(
            "client_llm.api_key 过长（上限 {} 字符）",
            CLIENT_LLM_API_KEY_MAX
        ));
    }
    Ok(Some(chat_job_queue::WebChatLlmOverride {
        api_base,
        model,
        api_key,
    }))
}

fn effective_llm_api_key_for_web_chat(
    state: &AppState,
    ov: &Option<chat_job_queue::WebChatLlmOverride>,
) -> String {
    if let Some(o) = ov
        && let Some(ref k) = o.api_key
        && !k.trim().is_empty()
    {
        return k.clone();
    }
    state.api_key.clone()
}

async fn ensure_bearer_api_key_for_chat(
    state: &AppState,
    llm_override: &Option<chat_job_queue::WebChatLlmOverride>,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    let auth = {
        let g = state.cfg.read().await;
        g.llm_http_auth_mode
    };
    if auth != LlmHttpAuthMode::Bearer {
        return Ok(());
    }
    let k = effective_llm_api_key_for_web_chat(state, llm_override);
    if k.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "LLM_API_KEY_REQUIRED",
                message: "当前为 bearer 鉴权但未配置 LLM API 密钥：请在侧栏「设置」中填写「API 密钥」（仅存本机浏览器），或设置环境变量 API_KEY 后重启服务。"
                    .to_string(),
            }),
        ));
    }
    Ok(())
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

/// 可选 `agent_role`：非空时与 `conversation_id` 同类字符约束，最长 64。
pub(crate) fn normalize_agent_role(raw: Option<&str>) -> Result<Option<String>, String> {
    const MAX: usize = 64;
    let Some(s) = raw.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    if s.len() > MAX {
        return Err(format!("agent_role 过长（最多 {MAX} 个字符）"));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("agent_role 仅允许字母、数字、- _ . :".to_string());
    }
    Ok(Some(s.to_string()))
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
    agent_role: Option<&str>,
) -> Result<ConversationTurnSeed, String> {
    if let Some(mut seed) = state.load_conversation_seed(conversation_id).await {
        seed.messages.push(Message::user_only(user_msg.to_string()));
        return Ok(seed);
    }
    // 先取工作区路径，再读 `cfg`，避免在持有 `cfg` 读锁时调用 `effective_workspace_path`（其内部再次 `cfg.read` 会死锁）。
    let root_str = state.effective_workspace_path().await;
    let cfg = state.cfg.read().await;
    let system_for_turn = cfg
        .system_prompt_for_new_conversation(agent_role)?
        .to_string();
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

    let want_heavy_scan = (cfg.project_profile_inject_enabled
        && cfg.project_profile_inject_max_chars > 0)
        || (cfg.project_dependency_brief_inject_enabled
            && cfg.project_dependency_brief_inject_max_chars > 0);
    let combined = if want_heavy_scan {
        let cfg_owned = cfg.clone();
        let root_scan = root.clone();
        match tokio::task::spawn_blocking(move || {
            build_first_turn_user_context_markdown(&root_scan, &cfg_owned, memory_snippet)
        })
        .await
        {
            Ok(v) => v,
            Err(e) => {
                debug!("first_turn_user_context spawn_blocking failed: {}", e);
                None
            }
        }
    } else {
        build_first_turn_user_context_markdown(&root, &cfg, memory_snippet)
    };

    let messages = match combined {
        Some(ctx) => vec![
            Message::system_only(system_for_turn.clone()),
            Message::user_only(ctx),
            Message::user_only(user_msg.to_string()),
        ],
        None => messages_chat_seed(&system_for_turn, user_msg),
    };
    Ok(ConversationTurnSeed {
        messages,
        expected_revision: None,
    })
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
    let token = {
        let g = state.cfg.read().await;
        g.web_api_bearer_token.expose_secret().trim().to_string()
    };
    if token.is_empty() {
        return next.run(req).await;
    }
    if is_valid_bearer_header(req.headers().get(header::AUTHORIZATION), token.as_str()) {
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
    let turn_seed = build_messages_for_turn(&state, &conversation_id, msg, agent_role.as_deref())
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
    let turn_seed = build_messages_for_turn(&state, &conversation_id, msg, agent_role.as_deref())
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
            llm_override,
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

/// `GET /workspace/changelog`：本会话工作区变更集 Markdown（与 **`session_workspace_changelist`** 注入正文同源）。
#[derive(serde::Deserialize)]
pub(crate) struct WorkspaceChangelogQuery {
    #[serde(default)]
    conversation_id: Option<String>,
}

#[derive(serde::Serialize)]
pub(crate) struct WorkspaceChangelogResponse {
    revision: u64,
    markdown: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

pub(crate) async fn workspace_changelog_handler(
    State(state): State<Arc<AppState>>,
    Query(q): Query<WorkspaceChangelogQuery>,
) -> Json<WorkspaceChangelogResponse> {
    let cid = match normalize_client_conversation_id(q.conversation_id.as_deref()) {
        Ok(o) => o,
        Err(msg) => {
            return Json(WorkspaceChangelogResponse {
                revision: 0,
                markdown: String::new(),
                error: Some(msg),
            });
        }
    };
    let scope = cid
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("__default__");
    let cfg = state.cfg.read().await;
    if !cfg.session_workspace_changelist_enabled {
        return Json(WorkspaceChangelogResponse {
            revision: 0,
            markdown: String::new(),
            error: Some(
                "会话工作区变更集已在配置中关闭（session_workspace_changelist_enabled）"
                    .to_string(),
            ),
        });
    }
    let max_chars = cfg.session_workspace_changelist_max_chars;
    drop(cfg);
    let cl = workspace_changelist::changelist_for_scope(scope);
    let (rev, body) = cl.snapshot_markdown(max_chars);
    Json(WorkspaceChangelogResponse {
        revision: rev,
        markdown: body.unwrap_or_default(),
        error: None,
    })
}

pub(crate) async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
    let (auth_mode, probe, probe_cache_secs, api_base) = {
        let g = state.cfg.read().await;
        (
            g.llm_http_auth_mode,
            g.health_llm_models_probe,
            g.health_llm_models_probe_cache_secs,
            g.api_base.clone(),
        )
    };
    let mut report = health::build_health_report(&work_dir, &state.api_key, auth_mode, true).await;
    health::append_llm_models_endpoint_probe(
        &mut report,
        health::LlmModelsEndpointProbeParams {
            enabled: probe,
            cache_secs: probe_cache_secs,
            cache_cell: state.llm_models_health_cache.as_ref(),
            client: &state.client,
            api_base: api_base.as_str(),
            api_key: state.api_key.as_str(),
            auth_mode,
        },
    )
    .await;
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
    /// CLI 是否在分阶段/逻辑双 agent 的**无工具规划轮**向 stdout 打印模型原文（默认 true）。
    staged_plan_cli_show_planner_stream: bool,
    /// 首轮规划后是否再跑无工具「步骤优化」轮（默认 true）。
    staged_plan_optimizer_round: bool,
    /// 逻辑多规划员份数上限（1–3，默认 1 即关闭）。
    staged_plan_ensemble_count: u8,
    /// SyncDefault 工具沙盒：`none` | `docker`。
    sync_default_tool_sandbox_mode: String,
    /// `docker` 模式下的镜像名（可能为空表示未启用或未配置）。
    sync_default_tool_sandbox_docker_image: String,
    /// Docker 沙盒容器进程身份摘要：`effective_uid:gid` | `image_default`（与配置 `current` / `image` 等对应）。
    sync_default_tool_sandbox_docker_user_effective: String,
    /// CLI REPL 是否在启动时从 `.crabmate/tui_session.json` 恢复会话（默认 false；文件名历史兼容）。
    tui_load_session_on_start: bool,
    /// CLI REPL 是否在后台构建 `initial_workspace_messages`（默认 false；仅 REPL）。
    repl_initial_workspace_messages_enabled: bool,
    max_message_history: usize,
    tool_message_max_chars: usize,
    context_char_budget: usize,
    context_summary_trigger_chars: usize,
    chat_queue_max_concurrent: usize,
    chat_queue_max_pending: usize,
    parallel_readonly_tools_max: usize,
    /// 单轮 `read_file` 缓存容量；`0` 表示关闭。
    read_file_turn_cache_max_entries: usize,
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
    /// 首轮是否追加 `cargo metadata` + package.json 的结构化摘要与 Mermaid workspace 图。
    project_dependency_brief_inject_enabled: bool,
    project_dependency_brief_inject_max_chars: usize,
    /// 是否要求非只读工具在 JSON 中带 `crabmate_explain_why`。
    tool_call_explain_enabled: bool,
    tool_call_explain_min_chars: usize,
    tool_call_explain_max_chars: usize,
    /// 自进程启动以来，同步上下文管道实际触发次数（累计，供排障；非「当前会话」）。
    message_pipeline_trim_count_hits: u64,
    message_pipeline_trim_char_budget_hits: u64,
    message_pipeline_tool_compress_hits: u64,
    message_pipeline_orphan_tool_drops: u64,
    /// 模型 HTTP 鉴权：`bearer` | `none`（如本地 Ollama 可不设 API_KEY）。
    llm_http_auth_mode: &'static str,
    /// 配置中的命名角色 id 列表（升序）；未启用多角色时为空。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    agent_role_ids: Vec<String>,
    /// Web/CLI 未指定 `agent_role` 时使用的默认角色 id（`null` 表示用全局 `system_prompt`）。
    #[serde(skip_serializing_if = "Option::is_none")]
    default_agent_role_id: Option<String>,
}

pub(crate) async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cfg = state.cfg.read().await;
    let mp = MESSAGE_PIPELINE_COUNTERS.snapshot();
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
    let mut agent_role_ids: Vec<String> = cfg.agent_roles.keys().cloned().collect();
    agent_role_ids.sort();
    Json(StatusResponse {
        status: "ok",
        model: cfg.model.clone(),
        api_base: cfg.api_base.clone(),
        max_tokens: cfg.max_tokens,
        temperature: cfg.temperature,
        llm_seed: cfg.llm_seed,
        tool_count: tool_names.len(),
        tool_names,
        tool_dispatch_registry: tool_registry::all_dispatch_metadata(),
        reflection_default_max_rounds: cfg.reflection_default_max_rounds,
        final_plan_requirement: cfg.final_plan_requirement,
        plan_rewrite_max_attempts: cfg.plan_rewrite_max_attempts,
        planner_executor_mode: cfg.planner_executor_mode.as_str(),
        staged_plan_execution: cfg.staged_plan_execution,
        staged_plan_cli_show_planner_stream: cfg.staged_plan_cli_show_planner_stream,
        staged_plan_optimizer_round: cfg.staged_plan_optimizer_round,
        staged_plan_ensemble_count: cfg.staged_plan_ensemble_count,
        sync_default_tool_sandbox_mode: cfg.sync_default_tool_sandbox_mode.as_str().to_string(),
        sync_default_tool_sandbox_docker_image: cfg.sync_default_tool_sandbox_docker_image.clone(),
        sync_default_tool_sandbox_docker_user_effective: match cfg
            .sync_default_tool_sandbox_docker_user
            .as_docker_user_string()
        {
            Some(s) => s.to_string(),
            None => "image_default".to_string(),
        },
        tui_load_session_on_start: cfg.tui_load_session_on_start,
        repl_initial_workspace_messages_enabled: cfg.repl_initial_workspace_messages_enabled,
        max_message_history: cfg.max_message_history,
        tool_message_max_chars: cfg.tool_message_max_chars,
        context_char_budget: cfg.context_char_budget,
        context_summary_trigger_chars: cfg.context_summary_trigger_chars,
        chat_queue_max_concurrent: state.chat_queue.max_concurrent(),
        chat_queue_max_pending: state.chat_queue.max_pending(),
        parallel_readonly_tools_max: cfg.parallel_readonly_tools_max,
        read_file_turn_cache_max_entries: cfg.read_file_turn_cache_max_entries,
        chat_queue_running: state.chat_queue.running_count(),
        chat_queue_completed_ok: state.chat_queue.completed_ok(),
        chat_queue_completed_cancelled: state.chat_queue.completed_cancelled(),
        chat_queue_completed_err: state.chat_queue.completed_err(),
        chat_queue_recent_jobs: state.chat_queue.recent_jobs(),
        per_active_jobs: state.chat_queue.active_per_jobs(),
        workspace_allowed_roots_count: cfg.workspace_allowed_roots.len(),
        conversation_store_entries,
        long_term_memory_enabled: cfg.long_term_memory_enabled,
        long_term_memory_vector_backend: cfg.long_term_memory_vector_backend.as_str().to_string(),
        long_term_memory_store_ready: ltm_ready,
        long_term_memory_index_errors: ltm_idx_err,
        project_profile_inject_enabled: cfg.project_profile_inject_enabled,
        project_profile_inject_max_chars: cfg.project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled: cfg.project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars: cfg.project_dependency_brief_inject_max_chars,
        tool_call_explain_enabled: cfg.tool_call_explain_enabled,
        tool_call_explain_min_chars: cfg.tool_call_explain_min_chars,
        tool_call_explain_max_chars: cfg.tool_call_explain_max_chars,
        message_pipeline_trim_count_hits: mp.trim_count_hits,
        message_pipeline_trim_char_budget_hits: mp.trim_char_budget_hits,
        message_pipeline_tool_compress_hits: mp.tool_compress_hits,
        message_pipeline_orphan_tool_drops: mp.orphan_tool_drops,
        llm_http_auth_mode: cfg.llm_http_auth_mode.as_str(),
        agent_role_ids,
        default_agent_role_id: cfg.default_agent_role_id.clone(),
    })
}

#[derive(serde::Serialize)]
pub(crate) struct ConfigReloadResponseBody {
    pub(crate) ok: bool,
    pub(crate) message: String,
}

/// 热重载 [`AgentConfig`] 可更字段（不含会话 SQLite 路径）；清空 MCP 进程缓存。
pub(crate) async fn config_reload_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ConfigReloadResponseBody>, (StatusCode, Json<ApiError>)> {
    let path = state.config_path_for_reload.as_deref();
    match crate::runtime::config_reload::reload_shared_agent_config(&state.cfg, path).await {
        Ok(()) => Ok(Json(ConfigReloadResponseBody {
            ok: true,
            message: "配置已热重载。conversation_store_sqlite_path 与 reqwest Client 未重建；若变更 web_api_bearer_token 是否启用中间件，须重启 serve。".to_string(),
        })),
        Err(e) => Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                code: "CONFIG_RELOAD_FAILED",
                message: e,
            }),
        )),
    }
}
