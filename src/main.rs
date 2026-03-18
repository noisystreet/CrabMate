//! 基于 DeepSeek API 的简易 Agent Demo
//! 支持工具调用、有限的 Linux 命令、流式输出；日志由 RUST_LOG 控制

mod api;
mod config;
mod tools;
mod types;
mod runtime;
mod ui;

use api::stream_chat;
use axum::{
    extract::Multipart,
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum::response::sse::{Event, KeepAlive, Sse};
use config::cli::{init_logging, parse_args};
use futures_util::StreamExt;
use tower::ServiceBuilder;
use tower_http::services::ServeDir;
use tower_http::set_header::SetResponseHeaderLayer;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use std::env;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info};
use types::{ChatRequest, Message};

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// 若提供 out，则流式 content 会通过 out 发送（供 SSE 等使用）。
/// 若 render_to_terminal 为 true，则在终端边收边打印；否则仅累积内容，供调用方统一渲染。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
pub(crate) async fn run_agent_turn(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &config::AgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    effective_working_dir: &std::path::Path,
    workspace_is_set: bool,
    render_to_terminal: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    'outer: loop {
        // 若用于 SSE 的发送端已关闭（通常是前端中止/断开连接），则尽快结束本轮对话与工具循环
        if let Some(tx) = out {
            if tx.is_closed() {
                info!("SSE sender closed, aborting run_agent_turn loop early");
                break;
            }
        }
        let req = ChatRequest {
            model: cfg.model.clone(),
            messages: messages.clone(),
            tools: Some(tools.to_vec()),
            tool_choice: Some("auto".to_string()),
            max_tokens: cfg.max_tokens,
            temperature: cfg.temperature,
            stream: None,
        };

        let t0 = Instant::now();
        let max_attempts = cfg.api_max_retries + 1;
        let mut msg_and_reason = None;
        for attempt in 0..max_attempts {
            match stream_chat(client, api_key, &cfg.api_base, &req, out, render_to_terminal).await {
                Ok(r) => {
                    info!(
                        model = %req.model,
                        elapsed_ms = t0.elapsed().as_millis(),
                        attempt = attempt + 1,
                        "chat 完成"
                    );
                    msg_and_reason = Some(r);
                    break;
                }
                Err(e) => {
                    error!(
                        error = %e,
                        attempt = attempt + 1,
                        max_attempts = max_attempts,
                        "API 请求失败"
                    );
                    if attempt < max_attempts - 1 {
                        let delay_secs = cfg.api_retry_delay_secs.saturating_mul(2_u64.saturating_pow(attempt as u32));
                        info!(delay_secs = delay_secs, "等待后重试");
                        tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    } else {
                        return Err(e.into());
                    }
                }
            }
        }
        let (msg, finish_reason) = msg_and_reason.expect("msg_and_reason 应在成功时赋值");
        messages.push(msg.clone());

        if finish_reason != "tool_calls" {
            break;
        }

        let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
        let mut workspace_changed = false;
        // 若有 SSE 输出通道，标记工具执行开始
        if let Some(tx) = out {
            let _ = tx.send(r#"{"tool_running":true}"#.to_string()).await;
        }
        for tc in tool_calls {
            if let Some(tx) = out {
                if tx.is_closed() {
                    info!("SSE sender closed during tool execution, aborting remaining tools");
                    break 'outer;
                }
            }
            let name = tc.function.name.clone();
            let args = tc.function.arguments.clone();
            let id = tc.id.clone();
            println!("  [调用工具: {}]", name);
            // 若有 SSE 输出通道，则先发送一条简短的工具调用摘要，供前端在 Chat 面板中展示
            if let Some(tx) = out {
                if let Some(summary) = tools::summarize_tool_call(&name, &args) {
                    let payload = serde_json::json!({
                        "tool_call": {
                            "name": name,
                            "summary": summary,
                        }
                    });
                    let _ = tx.send(payload.to_string()).await;
                }
            }
            let t_tool = Instant::now();
            let result = if name == "run_command" {
                if !workspace_is_set {
                    "错误：未设置工作区，禁止执行命令。请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。"
                        .to_string()
                } else {
                let name_in = name.clone();
                let cmd_timeout = cfg.command_timeout_secs;
                let cmd_max_len = cfg.command_max_output_len;
                let weather_secs = cfg.weather_timeout_secs;
                let allowed = cfg.allowed_commands.clone();
                let work_dir = effective_working_dir.to_path_buf();
                let args_cloned = args.clone();
                let handle = tokio::task::spawn_blocking(move || {
                    tools::run_tool(
                        &name_in,
                        &args_cloned,
                        cmd_max_len,
                        weather_secs,
                        &allowed,
                        &work_dir,
                    )
                });
                let s = match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        error!(tool = %name, error = ?e, "工具执行异常");
                        format!("工具执行异常：{:?}", e)
                    }
                    Err(_) => {
                        error!(tool = %name, "命令执行超时");
                        format!("命令执行超时（{} 秒）", cmd_timeout)
                    }
                };
                // 若是编译相关命令且退出码为 0，则认为工作区发生了变更（生成/更新了构建产物）
                if tools::is_compile_command_success(&args, &s) {
                    workspace_changed = true;
                }
                s
                }
            } else if name == "run_executable" {
                if !workspace_is_set {
                    "错误：未设置工作区，禁止运行可执行程序。请先在右侧工作区面板设置目录（可选择目录或手动输入路径）。"
                        .to_string()
                } else {
                let name_in = name.clone();
                let cmd_timeout = cfg.command_timeout_secs;
                let cmd_max_len = cfg.command_max_output_len;
                let weather_secs = cfg.weather_timeout_secs;
                let allowed = cfg.allowed_commands.clone();
                let work_dir = effective_working_dir.to_path_buf();
                let handle = tokio::task::spawn_blocking(move || {
                    tools::run_tool(
                        &name_in,
                        &args,
                        cmd_max_len,
                        weather_secs,
                        &allowed,
                        &work_dir,
                    )
                });
                match tokio::time::timeout(Duration::from_secs(cmd_timeout), handle).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        error!(tool = %name, error = ?e, "工具执行异常");
                        format!("工具执行异常：{:?}", e)
                    }
                    Err(_) => {
                        error!(tool = %name, "可执行程序运行超时");
                        format!("可执行程序运行超时（{} 秒）", cmd_timeout)
                    }
                }
                }
            } else if name == "get_weather" {
                let name_in = name.clone();
                let cmd_max_len = cfg.command_max_output_len;
                let weather_timeout = cfg.weather_timeout_secs;
                let allowed = cfg.allowed_commands.clone();
                let work_dir = effective_working_dir.to_path_buf();
                let handle = tokio::task::spawn_blocking(move || {
                    tools::run_tool(
                        &name_in,
                        &args,
                        cmd_max_len,
                        weather_timeout,
                        &allowed,
                        &work_dir,
                    )
                });
                match tokio::time::timeout(Duration::from_secs(weather_timeout), handle).await {
                    Ok(Ok(s)) => s,
                    Ok(Err(e)) => {
                        error!(tool = %name, error = ?e, "工具执行异常");
                        format!("工具执行异常：{:?}", e)
                    }
                    Err(_) => {
                        error!(tool = %name, "天气请求超时");
                        format!("天气请求超时（{} 秒）", weather_timeout)
                    }
                }
            } else {
                tools::run_tool(
                    &tc.function.name,
                    &tc.function.arguments,
                    cfg.command_max_output_len,
                    cfg.weather_timeout_secs,
                    &cfg.allowed_commands,
                    effective_working_dir,
                )
            };
            info!(tool = %name, elapsed_ms = t_tool.elapsed().as_millis(), "工具调用完成");
            // 若有 SSE 输出通道，将工具执行结果也发给前端，便于在 Chat 面板中展示，例如 ls 输出
            if let Some(tx) = out {
                let payload = serde_json::json!({
                    "tool_result": {
                        "name": name,
                        "output": result,
                    }
                });
                let _ = tx.send(payload.to_string()).await;
            }
            messages.push(Message {
                role: "tool".to_string(),
                content: Some(result),
                tool_calls: None,
                name: None,
                tool_call_id: Some(id),
            });
        }
        if let Some(tx) = out {
            if workspace_changed {
                let _ = tx.send(r#"{"workspace_changed":true}"#.to_string()).await;
            }
            // 工具执行结束
            let _ = tx.send(r#"{"tool_running":false}"#.to_string()).await;
        }
    }
    Ok(())
}

// ---------- Web 服务 ----------

#[derive(Clone)]
pub(crate) struct AppState {
    cfg: config::AgentConfig,
    api_key: String,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.run_command_working_dir
    workspace_override: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
    uploads_dir: std::path::PathBuf,
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
}

#[derive(serde::Deserialize)]
struct ChatRequestBody {
    message: String,
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
            error!(error = %e, dir = %dir.display(), "uploads 清理：无法读取目录");
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
        let ok = ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_' | ' ' | '(' | ')' | '[' | ']');
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
        let is_image = mime.starts_with("image/") && matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "gif");
        let is_audio = mime.starts_with("audio/") && matches!(ext.as_str(), "mp3" | "wav" | "m4a" | "aac" | "ogg" | "webm");
        let is_video = mime.starts_with("video/") && matches!(ext.as_str(), "mp4" | "webm" | "mov" | "mkv");
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

        let ext_with_dot = if ext.is_empty() { "".to_string() } else { format!(".{}", ext) };

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
            let Some(chunk) = next else { break; };
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
}

/// 统一的 API 错误结构：包含错误码与面向用户的友好提示
#[derive(serde::Serialize)]
struct ApiError {
    /// 机器可读的错误码（前端或日志可用）
    code: &'static str,
    /// 面向用户展示的友好错误信息
    message: String,
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
    let mut messages: Vec<Message> = vec![
        Message {
            role: "system".to_string(),
            content: Some(state.cfg.system_prompt.clone()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some(msg.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    if messages.len() > 1 + state.cfg.max_message_history {
        let keep = messages.len() - state.cfg.max_message_history;
        messages = [messages[0].clone()]
            .into_iter()
            .chain(messages.into_iter().skip(keep))
            .collect();
    }
    let work_dir_str = state.effective_workspace_path().await;
    let work_dir = std::path::Path::new(&work_dir_str);
    let workspace_is_set = state.workspace_is_set().await;
    run_agent_turn(
        &state.client,
        &state.api_key,
        &state.cfg,
        &state.tools,
        &mut messages,
        None,
        work_dir,
        workspace_is_set,
        true,
    )
    .await
    .map_err(|e| {
        // 记录具体错误到日志，但对前端仅暴露通用友好文案与错误码
        error!(error = %e, "chat_handler 调用 run_agent_turn 失败");
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
    Ok(Json(ChatResponseBody { reply }))
}

/// 流式 chat：返回 SSE，每个 event 的 data 为一段 content delta（或结束时一条 error JSON）
async fn chat_stream_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ChatRequestBody>,
) -> Result<Sse<impl futures_util::Stream<Item = Result<Event, std::convert::Infallible>>>, (StatusCode, Json<ApiError>)> {
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
    let mut messages: Vec<Message> = vec![
        Message {
            role: "system".to_string(),
            content: Some(state.cfg.system_prompt.clone()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "user".to_string(),
            content: Some(msg.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    ];
    if messages.len() > 1 + state.cfg.max_message_history {
        let keep = messages.len() - state.cfg.max_message_history;
        messages = [messages[0].clone()]
            .into_iter()
            .chain(messages.into_iter().skip(keep))
            .collect();
    }
    let (tx, rx) = mpsc::channel::<String>(1024);
    let state_clone = state.clone();
    tokio::spawn(async move {
        let work_dir = std::path::PathBuf::from(state_clone.effective_workspace_path().await);
        let workspace_is_set = state_clone.workspace_is_set().await;
        let out = Some(&tx);
        if let Err(e) = run_agent_turn(
            &state_clone.client,
            &state_clone.api_key,
            &state_clone.cfg,
            &state_clone.tools,
            &mut messages,
            out,
            &work_dir,
            workspace_is_set,
            false,
        )
        .await
        {
            // 将具体错误记录在日志中，仅通过统一的错误码和友好提示回传给前端
            error!(error = %e, "chat_stream_handler 中 run_agent_turn 失败");
            let err_json = serde_json::json!({
                "error": "对话失败，请稍后重试",
                "code": "INTERNAL_ERROR",
            });
            let _ = tx.send(err_json.to_string()).await;
        }
        drop(tx);
    });
    let stream = ReceiverStream::new(rx).map(|s| Ok(Event::default().data(s)));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

#[derive(serde::Serialize)]
struct HealthCheckItem {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(serde::Serialize)]
struct HealthResponse {
    /// ok / degraded
    status: &'static str,
    checks: std::collections::BTreeMap<&'static str, HealthCheckItem>,
}

async fn health_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut checks: std::collections::BTreeMap<&'static str, HealthCheckItem> =
        std::collections::BTreeMap::new();

    // API key
    let api_key_ok = !state.api_key.trim().is_empty();
    checks.insert(
        "api_key",
        HealthCheckItem {
            ok: api_key_ok,
            detail: if api_key_ok {
                None
            } else {
                Some("未设置 API_KEY".to_string())
            },
        },
    );

    // frontend static dir (optional for --no-web)
    let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
    let static_ok = static_dir.is_dir();
    checks.insert(
        "frontend_static_dir",
        HealthCheckItem {
            ok: static_ok,
            detail: if static_ok {
                None
            } else {
                Some(format!("目录不存在：{}", static_dir.display()))
            },
        },
    );

    // workspace writable (create + delete temp file)
    let work_dir = std::path::PathBuf::from(state.effective_workspace_path().await);
    let writable = tokio::task::spawn_blocking({
        let work_dir = work_dir.clone();
        move || {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let pid = std::process::id();
            let p = work_dir.join(format!(".crabmate_healthcheck_{}_{}.tmp", pid, ts));
            match std::fs::write(&p, b"") {
                Ok(()) => {
                    let _ = std::fs::remove_file(&p);
                    Ok(())
                }
                Err(e) => Err(e),
            }
        }
    })
    .await
    .ok()
    .and_then(|r| r.err())
    .map(|e| format!("不可写：{}（{}）", work_dir.display(), e));
    checks.insert(
        "workspace_writable",
        HealthCheckItem {
            ok: writable.is_none(),
            detail: writable,
        },
    );

    // executable dependencies
    let deps = tokio::task::spawn_blocking(|| {
        fn check_cmd(cmd: &str, args: &[&str]) -> Result<String, String> {
            match std::process::Command::new(cmd).args(args).output() {
                Ok(out) => {
                    let status = out.status.code().unwrap_or(-1);
                    if status == 0 {
                        let s = if !out.stdout.is_empty() {
                            String::from_utf8_lossy(&out.stdout).trim().to_string()
                        } else {
                            String::from_utf8_lossy(&out.stderr).trim().to_string()
                        };
                        Ok(if s.is_empty() {
                            "ok".to_string()
                        } else {
                            s
                        })
                    } else {
                        Err(format!("exit={}", status))
                    }
                }
                Err(e) => Err(e.to_string()),
            }
        }

        let mut m = std::collections::BTreeMap::new();

        // bc: GNU bc 有时支持 --version，也可能是 -v/-V；尽量多试几种
        let bc = check_cmd("bc", &["--version"])
            .or_else(|_| check_cmd("bc", &["-v"]))
            .or_else(|_| check_cmd("bc", &["-V"]));
        m.insert("bc", bc);

        let rustfmt = check_cmd("rustfmt", &["--version"]);
        m.insert("rustfmt", rustfmt);

        let npm = check_cmd("npm", &["--version"]);
        m.insert("npm", npm);

        m
    })
    .await
    .ok()
    .unwrap_or_default();

    for (k, v) in deps {
        let key: &'static str = match k {
            "bc" => "dep_bc",
            "rustfmt" => "dep_rustfmt",
            "npm" => "dep_npm",
            _ => continue,
        };
        match v {
            Ok(detail) => {
                checks.insert(
                    key,
                    HealthCheckItem {
                        ok: true,
                        detail: Some(detail),
                    },
                );
            }
            Err(err) => {
                checks.insert(
                    key,
                    HealthCheckItem {
                        ok: false,
                        detail: Some(err),
                    },
                );
            }
        }
    }

    let required_ok = checks
        .get("api_key")
        .map(|c| c.ok)
        .unwrap_or(false)
        && checks
            .get("workspace_writable")
            .map(|c| c.ok)
            .unwrap_or(false);
    let status = if required_ok && checks.values().all(|c| c.ok) {
        "ok"
    } else if required_ok {
        "degraded"
    } else {
        "degraded"
    };

    Json(HealthResponse { status, checks })
}

#[derive(serde::Serialize)]
struct StatusResponse {
    status: &'static str,
    model: String,
    api_base: String,
    max_tokens: u32,
    temperature: f32,
}

async fn status_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    Json(StatusResponse {
        status: "ok",
        model: state.cfg.model.clone(),
        api_base: state.cfg.api_base.clone(),
        max_tokens: state.cfg.max_tokens,
        temperature: state.cfg.temperature,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();

    let (
        config_path,
        single_shot,
        serve_port,
        workspace_cli,
        output_mode,
        no_tools,
        no_web,
        dry_run,
        no_stream,
        tui,
    ) = parse_args();

    let api_key = env::var("API_KEY")
        .expect("请设置环境变量 API_KEY");

    let cfg = match config::load_config(config_path.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };
    info!(api_base = %cfg.api_base, model = %cfg.model, "配置已加载");
    if dry_run {
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        if !static_dir.is_dir() {
            eprintln!(
                "dry-run 失败：前端静态目录不存在：{}（请先在 frontend/ 下构建）",
                static_dir.display()
            );
            std::process::exit(1);
        }
        println!("配置检查通过：API_KEY 已设置，配置可用，前端静态目录存在：{}", static_dir.display());
        return Ok(());
    }
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.api_timeout_secs))
        .build()?;
    let all_tools = tools::build_tools();
    let tools = if no_tools { Vec::new() } else { all_tools };

    if tui {
        crate::runtime::tui::run_tui(
            &cfg,
            &client,
            &api_key,
            &tools,
            &workspace_cli,
            no_stream,
        )
        .await?;
        return Ok(());
    }

    if let Some(port) = serve_port {
        let initial_workspace = workspace_cli.clone();
            let uploads_dir = std::env::temp_dir().join("crabmate_uploads");
            std::fs::create_dir_all(&uploads_dir).ok();
        let state = Arc::new(AppState {
            cfg: cfg.clone(),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
                uploads_dir: uploads_dir.clone(),
        });
            let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
            let uploads_dir_for_static = uploads_dir.clone();
        let mut app = Router::new()
            .route("/chat", post(chat_handler))
            .route("/chat/stream", post(chat_stream_handler))
                .route("/upload", post(upload_handler))
                .route("/uploads/delete", post(delete_uploads_handler))
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
                )
            .route(
                "/workspace",
                get(ui::workspace::workspace_handler).post(ui::workspace::workspace_set_handler),
            )
            .route("/workspace/pick", get(ui::workspace::workspace_pick_handler))
            .route(
                "/workspace/search",
                post(ui::workspace::workspace_search_handler),
            )
            .route(
                "/workspace/file",
                get(ui::workspace::workspace_file_read_handler)
                    .post(ui::workspace::workspace_file_write_handler)
                    .delete(ui::workspace::workspace_file_delete_handler),
            )
            .route(
                "/tasks",
                get(ui::task::tasks_get_handler).post(ui::task::tasks_set_handler),
            );
        if !no_web {
            app = app.nest_service("/", ServeDir::new(static_dir));
        }
        let app = app.with_state(state);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        println!("Web 服务已启动");
        println!("  本地访问: http://127.0.0.1:{}", port);
        println!("  监听地址: http://0.0.0.0:{}", port);
        info!(port = %port, "Web 服务监听 http://{}", addr);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        // uploads 自动清理：每 10 分钟执行一次；保留 24h；总容量上限 500MB
        tokio::spawn({
            let dir = uploads_dir.clone();
            async move {
                let mut interval = tokio::time::interval(Duration::from_secs(600));
                loop {
                    interval.tick().await;
                    cleanup_uploads_dir(dir.clone(), Duration::from_secs(24 * 3600), 500 * 1024 * 1024).await;
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

    crate::runtime::cli::run_repl(
        &cfg,
        &client,
        &api_key,
        &tools,
        &workspace_cli,
        no_stream,
    )
    .await
}
