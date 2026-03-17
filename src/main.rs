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
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use axum::response::sse::{Event, KeepAlive, Sse};
use config::cli::{init_logging, parse_args};
use futures_util::StreamExt;
use tower_http::services::ServeDir;
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

    if let Some(port) = serve_port {
        let initial_workspace = workspace_cli.clone();
        let state = Arc::new(AppState {
            cfg: cfg.clone(),
            api_key: api_key.clone(),
            client,
            tools,
            workspace_override: std::sync::Arc::new(tokio::sync::RwLock::new(initial_workspace)),
        });
        let static_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("frontend/dist");
        let mut app = Router::new()
            .route("/chat", post(chat_handler))
            .route("/chat/stream", post(chat_stream_handler))
            .route("/health", get(health_handler))
            .route("/status", get(status_handler))
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
