//! 基于 DeepSeek API 的简易 Agent Demo
//! 支持工具调用、有限的 Linux 命令、流式输出；日志由 RUST_LOG 控制

mod api;
mod config;
mod tools;
mod types;

use api::stream_chat;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::StreamExt;
use tower_http::services::ServeDir;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use std::env;
use std::io::{self, Read, Write};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info};
use types::{ChatRequest, Message};

fn init_logging() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();
}

/// 若命令行含 --help / -h，打印使用说明并退出。
fn maybe_print_help_and_exit() {
    let args: Vec<String> = env::args().collect();
    for a in args.iter().skip(1) {
        if a == "--help" || a == "-h" {
            print_usage();
            std::process::exit(0);
        }
    }
}

fn print_usage() {
    let prog = env::args().next().unwrap_or_else(|| "agent_demo".to_string());
    eprintln!(r#"DeepSeek Agent Demo - 基于 DeepSeek API 的简易 Agent，支持工具调用与流式输出

用法:
  {} [选项]

选项:
  -h, --help           显示此帮助信息
  --config <path>      指定配置文件（覆盖默认的 config.toml / .agent_demo.toml）
  --serve [port]       以 Web 服务启动，默认端口 8080（POST /chat 提问，GET /health 健康检查）
  --query <问题>        单次提问，输出回答后退出（便于脚本调用）
  --stdin              从标准输入读取问题（多行直到 EOF），输出回答后退出（便于管道）

环境变量:
  API_KEY              必填，DeepSeek API Key（见 https://platform.deepseek.com/）
  RUST_LOG             可选，日志级别，如 info、agent_demo=debug
  AGENT_*              可选，覆盖配置项，如 AGENT_MODEL、AGENT_API_BASE、AGENT_MAX_TOKENS 等

配置:
  默认从 default_config.toml（嵌入）+ config.toml 或 .agent_demo.toml + 环境变量 合并。
  使用 --config 时仅从该文件合并，不再查找当前目录下的 config.toml。

示例:
  export API_KEY=your-key
  {}
  {} --config ./my.toml
  {} --query "北京今天天气怎么样"
  echo "1+1等于几" | {} --stdin
  {} --serve
  {} --serve 3000
  RUST_LOG=info {} --config ./prod.toml
"#, prog, prog, prog, prog, prog, prog, prog, prog);
}

/// 从标准输入读取全部内容（直到 EOF）
fn read_stdin_to_string() -> String {
    let mut s = String::new();
    let _ = io::stdin().read_to_string(&mut s);
    s
}

/// 解析命令行参数，返回 (--config 路径, 单次问题, --serve 端口)
fn parse_args() -> (Option<String>, Option<String>, Option<u16>) {
    let args: Vec<String> = env::args().collect();
    let mut config_path = None;
    let mut single_shot = None;
    let mut serve_port = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--config" {
            i += 1;
            if i < args.len() {
                config_path = Some(args[i].clone());
            }
            i += 1;
        } else if args[i] == "--serve" {
            i += 1;
            let port = if i < args.len() {
                args[i].parse::<u16>().ok()
            } else {
                None
            };
            serve_port = Some(port.unwrap_or(8080));
            if port.is_some() {
                i += 1;
            }
        } else if args[i] == "--query" {
            i += 1;
            if i < args.len() {
                single_shot = Some(args[i].clone());
            }
            i += 1;
        } else if args[i] == "--stdin" {
            i += 1;
            single_shot = Some(read_stdin_to_string());
        } else {
            i += 1;
        }
    }
    (config_path, single_shot, serve_port)
}

/// 执行一轮 Agent：发请求、若遇 tool_calls 则执行工具并继续，直到模型返回最终回复。
/// 若提供 out，则流式 content 会通过 out 发送（供 SSE 等使用）。
/// effective_working_dir 为当前生效的工作目录（可与前端设置的工作区一致）。
async fn run_agent_turn(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &config::AgentConfig,
    tools: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    effective_working_dir: &std::path::Path,
    workspace_is_set: bool,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    loop {
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
            match stream_chat(client, api_key, &cfg.api_base, &req, out).await {
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
            let name = tc.function.name.clone();
            let args = tc.function.arguments.clone();
            let id = tc.id.clone();
            println!("  [调用工具: {}]", name);
            // 若有 SSE 输出通道，则先发送一条简短的工具调用摘要，供前端在 Chat 面板中展示
            if let Some(tx) = out {
                if let Some(summary) = summarize_tool_call(&name, &args) {
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
                if is_compile_command_success(&args, &s) {
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

/// 判断本次 run_command 是否为“成功的编译命令”（gcc/g++/make/cmake 且退出码为 0）
fn is_compile_command_success(args_json: &str, result: &str) -> bool {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(_) => return false,
        };
    let cmd = v
        .get("command")
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_lowercase());
    let is_compile_cmd = cmd
        .as_deref()
        .map_or(false, |c| matches!(c, "gcc" | "g++" | "make" | "cmake"));
    if !is_compile_cmd {
        return false;
    }
    // run_command 输出的第一行形如：退出码：0
    let first_line = result.lines().next().unwrap_or("");
    if let Some(rest) = first_line.strip_prefix("退出码：") {
        if let Ok(code) = rest.trim().parse::<i32>() {
            return code == 0;
        }
    }
    false
}

/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    match name {
        "run_command" => {
            let cmd = v.get("command")?.as_str()?.trim();
            let args = v
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let s = if args.is_empty() {
                format!("执行命令：{}", cmd)
            } else {
                format!("执行命令：{} {}", cmd, args)
            };
            Some(s)
        }
        "create_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("新建文件：{}", path))
        }
        "modify_file" => {
            let path = v.get("path")?.as_str()?.trim();
            Some(format!("修改文件：{}", path))
        }
        "run_executable" => {
            let path = v.get("path")?.as_str()?.trim();
            let args = v
                .get("args")
                .and_then(|a| a.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default();
            let s = if args.is_empty() {
                format!("运行可执行：{}", path)
            } else {
                format!("运行可执行：{} {}", path, args)
            };
            Some(s)
        }
        _ => None,
    }
}

// ---------- Web 服务 ----------

#[derive(Clone)]
struct AppState {
    cfg: config::AgentConfig,
    api_key: String,
    client: reqwest::Client,
    tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.run_command_working_dir
    workspace_override: std::sync::Arc<tokio::sync::RwLock<Option<String>>>,
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

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
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

#[derive(serde::Serialize)]
struct WorkspaceEntry {
    name: String,
    is_dir: bool,
}

#[derive(serde::Deserialize)]
struct WorkspaceQuery {
    path: Option<String>,
}

#[derive(serde::Serialize)]
struct WorkspaceResponse {
    path: String,
    entries: Vec<WorkspaceEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn resolve_workspace_path(
    base: &std::path::Path,
    sub: Option<&str>,
) -> Result<std::path::PathBuf, String> {
    let sub = match sub {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => return Ok(base.to_path_buf()),
    };
    let joined = if std::path::Path::new(sub).is_absolute() {
        std::path::PathBuf::from(sub)
    } else {
        base.join(sub)
    };
    let canonical = joined.canonicalize().map_err(|e| format!("路径无法解析: {}", e))?;
    Ok(canonical)
}

#[derive(serde::Deserialize)]
struct WorkspaceSetBody {
    path: Option<String>,
}

async fn workspace_set_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceSetBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let raw = body.path.as_deref().map(|s| s.trim()).unwrap_or("");
    let path = if raw.is_empty() { "" } else { raw };
    let mut guard = state.workspace_override.write().await;
    // None 表示“从未设置过”；Some("") 表示“显式选择默认目录”；Some("...") 表示指定路径
    *guard = Some(path.to_string());
    Ok(Json(serde_json::json!({ "ok": true, "path": path })))
}

async fn workspace_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceQuery>,
) -> Json<WorkspaceResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = std::path::Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            let msg = format!("工作目录无法解析: {}", e);
            tracing::warn!("{}", msg);
            return Json(WorkspaceResponse {
                path: String::new(),
                entries: Vec::new(),
                error: Some(msg),
            });
        }
    };
    let canonical = match resolve_workspace_path(&base_canonical, query.path.as_deref()) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceResponse {
                path: base_canonical.display().to_string(),
                entries: Vec::new(),
                error: Some(msg),
            });
        }
    };
    let path_str = canonical.display().to_string();
    let mut entries = Vec::new();
    let mut read_dir = match tokio::fs::read_dir(&canonical).await {
        Ok(d) => d,
        Err(e) => {
            let msg = format!("无法读取工作目录: {}", e);
            tracing::warn!("{}", msg);
            return Json(WorkspaceResponse {
                path: path_str,
                entries: Vec::new(),
                error: Some(msg),
            });
        }
    };
    loop {
        let entry = match read_dir.next_entry().await {
            Ok(Some(e)) => e,
            Ok(None) => break,
            Err(e) => {
                let msg = format!("读取目录项失败: {}", e);
                tracing::warn!("{}", msg);
                break;
            }
        };
        let name = entry
            .file_name()
            .to_string_lossy()
            .to_string();
        let is_dir = entry.metadata().await.map(|m| m.is_dir()).unwrap_or(false);
        entries.push(WorkspaceEntry { name, is_dir });
    }
    entries.sort_by(|a, b| {
        match (a.is_dir, b.is_dir) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });
    Json(WorkspaceResponse {
        path: path_str,
        entries,
        error: None,
    })
}

#[derive(serde::Serialize)]
struct WorkspacePickResponse {
    path: Option<String>,
}

async fn workspace_pick_handler() -> Json<WorkspacePickResponse> {
    let path = tokio::task::spawn_blocking(|| {
        rfd::FileDialog::new().pick_folder()
    })
    .await
    .ok()
    .and_then(|opt| opt)
    .map(|p| p.display().to_string());
    Json(WorkspacePickResponse { path })
}

#[derive(serde::Deserialize)]
struct WorkspaceFileQuery {
    path: String,
}

#[derive(serde::Serialize)]
struct WorkspaceFileReadResponse {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// 工作区文件读取：按 path 返回文件内容（path 为工作区内文件路径）
async fn workspace_file_read_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileReadResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = std::path::Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    let path = query.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_workspace_path(&base_canonical, Some(path)) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(msg),
            });
        }
    };
    let meta = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(e) => {
            return Json(WorkspaceFileReadResponse {
                content: String::new(),
                error: Some(format!("无法读取文件信息: {}", e)),
            });
        }
    };
    if meta.is_dir() {
        return Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some("路径是目录，无法读取为文件".to_string()),
        });
    }
    match tokio::fs::read_to_string(&canonical).await {
        Ok(content) => Json(WorkspaceFileReadResponse {
            content,
            error: None,
        }),
        Err(e) => Json(WorkspaceFileReadResponse {
            content: String::new(),
            error: Some(format!("读取文件失败: {}", e)),
        }),
    }
}

#[derive(serde::Deserialize)]
struct WorkspaceFileWriteBody {
    path: String,
    content: String,
    /// 仅创建：若文件已存在则报错
    #[serde(default)]
    create_only: bool,
    /// 仅修改：若文件不存在则报错
    #[serde(default)]
    update_only: bool,
}

#[derive(serde::Serialize)]
struct WorkspaceFileWriteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// 工作区文件写入：支持创建、写入（创建或覆盖）、仅创建、仅修改
async fn workspace_file_write_handler(
    State(state): State<Arc<AppState>>,
    Json(body): Json<WorkspaceFileWriteBody>,
) -> Json<WorkspaceFileWriteResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = std::path::Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileWriteResponse {
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    let path = body.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileWriteResponse {
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_workspace_path(&base_canonical, Some(path)) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceFileWriteResponse {
                error: Some(msg),
            });
        }
    };

    let exists = tokio::fs::try_exists(&canonical).await.unwrap_or(false);
    if body.create_only && exists {
        return Json(WorkspaceFileWriteResponse {
            error: Some("文件已存在，无法仅创建".to_string()),
        });
    }
    if body.update_only && !exists {
        return Json(WorkspaceFileWriteResponse {
            error: Some("文件不存在，无法仅修改".to_string()),
        });
    }

    if let Some(parent) = canonical.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return Json(WorkspaceFileWriteResponse {
                    error: Some(format!("创建目录失败: {}", e)),
                });
            }
        }
    }
    match tokio::fs::write(&canonical, body.content.as_bytes()).await {
        Ok(()) => Json(WorkspaceFileWriteResponse { error: None }),
        Err(e) => Json(WorkspaceFileWriteResponse {
            error: Some(format!("写入文件失败: {}", e)),
        }),
    }
}

#[derive(serde::Serialize)]
struct WorkspaceFileDeleteResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// 删除工作区内的文件：path 为工作区内文件路径，不能删除目录
async fn workspace_file_delete_handler(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WorkspaceFileQuery>,
) -> Json<WorkspaceFileDeleteResponse> {
    let base_str = state.effective_workspace_path().await;
    let base = std::path::Path::new(&base_str);
    let base_canonical = match base.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Json(WorkspaceFileDeleteResponse {
                error: Some(format!("工作目录无法解析: {}", e)),
            });
        }
    };
    let path = query.path.trim();
    if path.is_empty() {
        return Json(WorkspaceFileDeleteResponse {
            error: Some("path 不能为空".to_string()),
        });
    }
    let canonical = match resolve_workspace_path(&base_canonical, Some(path)) {
        Ok(p) => p,
        Err(msg) => {
            return Json(WorkspaceFileDeleteResponse {
                error: Some(msg),
            });
        }
    };
    let meta = match tokio::fs::metadata(&canonical).await {
        Ok(m) => m,
        Err(e) => {
            return Json(WorkspaceFileDeleteResponse {
                error: Some(format!("无法读取文件信息: {}", e)),
            });
        }
    };
    if meta.is_dir() {
        return Json(WorkspaceFileDeleteResponse {
            error: Some("不支持删除目录".to_string()),
        });
    }
    match tokio::fs::remove_file(&canonical).await {
        Ok(()) => Json(WorkspaceFileDeleteResponse { error: None }),
        Err(e) => Json(WorkspaceFileDeleteResponse {
            error: Some(format!("删除文件失败: {}", e)),
        }),
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct TaskItem {
    id: String,
    title: String,
    done: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct TasksData {
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    updated_at: Option<String>,
    items: Vec<TaskItem>,
}

/// 读取当前工作区根目录下的 tasks.json；若不存在则返回空任务列表
async fn tasks_get_handler(State(state): State<Arc<AppState>>) -> Json<TasksData> {
    let base_str = state.effective_workspace_path().await;
    let root = std::path::Path::new(&base_str);
    let path = root.join("tasks.json");
    if !path.exists() {
        return Json(TasksData {
            source: None,
            updated_at: None,
            items: Vec::new(),
        });
    }
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => match serde_json::from_str::<TasksData>(&s) {
            Ok(data) => Json(data),
            Err(e) => {
                error!(error = %e, "解析 tasks.json 失败，将返回空任务列表");
                Json(TasksData {
                    source: None,
                    updated_at: None,
                    items: Vec::new(),
                })
            }
        },
        Err(e) => {
            error!(error = %e, "读取 tasks.json 失败，将返回空任务列表");
            Json(TasksData {
                source: None,
                updated_at: None,
                items: Vec::new(),
            })
        }
    }
}

/// 覆盖写入当前工作区根目录的 tasks.json
async fn tasks_set_handler(
    State(state): State<Arc<AppState>>,
    Json(mut body): Json<TasksData>,
) -> Json<TasksData> {
    let base_str = state.effective_workspace_path().await;
    let root = std::path::Path::new(&base_str);
    let path = root.join("tasks.json");
    // 由后端统一维护更新时间
    let now = chrono::Utc::now().to_rfc3339();
    body.updated_at = Some(now);
    let content = match serde_json::to_string_pretty(&body) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "序列化任务数据失败");
            return Json(body);
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            error!(error = %e, "创建 tasks.json 目录失败");
        }
    }
    if let Err(e) = tokio::fs::write(&path, content.as_bytes()).await {
        error!(error = %e, "写入 tasks.json 失败");
    }
    Json(body)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    maybe_print_help_and_exit();
    init_logging();

    let (config_path, single_shot, serve_port) = parse_args();

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
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(cfg.api_timeout_secs))
        .build()?;
    let tools = tools::build_tools();

    if let Some(port) = serve_port {
        let state = Arc::new(AppState {
            cfg: cfg.clone(),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(None)),
        });
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        let app = Router::new()
            .route("/chat", post(chat_handler))
            .route("/chat/stream", post(chat_stream_handler))
            .route("/health", get(health_handler))
            .route("/status", get(status_handler))
            .route("/workspace", get(workspace_handler).post(workspace_set_handler))
            .route("/workspace/pick", get(workspace_pick_handler))
            .route(
                "/workspace/file",
                get(workspace_file_read_handler)
                    .post(workspace_file_write_handler)
                    .delete(workspace_file_delete_handler),
            )
            .route("/tasks", get(tasks_get_handler).post(tasks_set_handler))
            .nest_service("/", ServeDir::new(static_dir))
            .with_state(state);
        let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
        println!("Web 服务已启动");
        println!("  本地访问: http://127.0.0.1:{}", port);
        println!("  监听地址: http://0.0.0.0:{}", port);
        info!(port = %port, "Web 服务监听 http://{}", addr);
        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;
        return Ok(());
    }

    let mut messages: Vec<Message> = vec![Message {
        role: "system".to_string(),
        content: Some(cfg.system_prompt.clone()),
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }];

    if let Some(question) = single_shot {
        let q = question.trim();
        if q.is_empty() {
            eprintln!("错误：--query 或 --stdin 内容为空");
            std::process::exit(1);
        }
        messages.push(Message {
            role: "user".to_string(),
            content: Some(q.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });
        if messages.len() > 1 + cfg.max_message_history {
            let keep = messages.len() - cfg.max_message_history;
            messages = [messages[0].clone()]
                .into_iter()
                .chain(messages.into_iter().skip(keep))
                .collect();
        }
        if let Err(e) = run_agent_turn(
            &client,
            &api_key,
            &cfg,
            &tools,
            &mut messages,
            None,
            std::path::Path::new(&cfg.run_command_working_dir),
            true,
        )
        .await
        {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return Ok(());
    }

    println!("=== DeepSeek Agent Demo ===\n当前模型: {}\n输入内容与 Agent 对话，输入 quit/exit 或 Ctrl+D 退出。\n", cfg.model);

    loop {
        print!("你: ");
        io::stdout().flush()?;
        let mut input = String::new();
        let n = io::stdin().read_line(&mut input)?;
        if n == 0 {
            break; // Ctrl+D (EOF)
        }
        let input = input.trim();
        if input.eq_ignore_ascii_case("quit") || input.eq_ignore_ascii_case("exit") {
            break;
        }
        if input.is_empty() {
            continue;
        }

        messages.push(Message {
            role: "user".to_string(),
            content: Some(input.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        });

        if messages.len() > 1 + cfg.max_message_history {
            let keep = messages.len() - cfg.max_message_history;
            messages = [messages[0].clone()]
                .into_iter()
                .chain(messages.into_iter().skip(keep))
                .collect();
        }

        if let Err(e) = run_agent_turn(
            &client,
            &api_key,
            &cfg,
            &tools,
            &mut messages,
            None,
            std::path::Path::new(&cfg.run_command_working_dir),
            true,
        )
        .await
        {
            eprintln!("{}", e);
            break;
        }
    }

    println!("再见。");
    Ok(())
}
