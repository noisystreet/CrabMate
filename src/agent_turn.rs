//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」对齐的调用边界（P/E/R）。
//! 被 crate 根 `run_agent_turn`（Web）与 `run_agent_turn_tui`（TUI）共同复用。

use std::path::Path;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;
use tracing::{error, info};

use crate::api::stream_chat;
use crate::config::AgentConfig;
use crate::per_coord::PerCoordinator;
use crate::sse_protocol::{SsePayload, ToolCallSummary, ToolResultBody, encode_message};
use crate::tool_registry::{self, ToolRuntime};
use crate::tools;
use crate::types::{ChatRequest, Message, ToolCall};

// --- P：向模型要本轮输出（含重试）---

/// P：构造请求并调用模型（`no_stream` 为 true 时走 `stream: false`），**不**修改 `messages`。
pub(crate) async fn per_plan_call_model_retrying(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &AgentConfig,
    tools_defs: &[crate::types::Tool],
    messages: &[Message],
    out: Option<&mpsc::Sender<String>>,
    render_to_terminal: bool,
    no_stream: bool,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
        tools: Some(tools_defs.to_vec()),
        tool_choice: Some("auto".to_string()),
        max_tokens: cfg.max_tokens,
        temperature: cfg.temperature,
        stream: None,
    };

    let t0 = Instant::now();
    let max_attempts = cfg.api_max_retries + 1;
    let mut msg_and_reason = None;
    for attempt in 0..max_attempts {
        match stream_chat(
            client,
            api_key,
            &cfg.api_base,
            &req,
            out,
            render_to_terminal,
            no_stream,
        )
        .await
        {
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
                    let delay_secs = cfg
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt));
                    info!(delay_secs = delay_secs, "等待后重试");
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                } else {
                    return Err(e);
                }
            }
        }
    }
    msg_and_reason.ok_or_else(|| {
        std::io::Error::other("chat 请求成功但未拿到消息内容（msg_and_reason 为空）").into()
    })
}

// --- R：终答阶段（须含规划等）---

/// R：模型本轮若为最终文本（非 tool_calls），决定是否结束或追加重写提示。
pub(crate) enum ReflectOnAssistantOutcome {
    /// 结束 `run_agent_turn` 外层循环
    StopTurn,
    /// 已写入重写 user 消息，应继续外层循环再次请求模型
    ContinueOuterForPlanRewrite,
    /// 进入工具执行阶段
    ProceedToExecuteTools,
}

pub(crate) fn per_reflect_after_assistant(
    per_coord: &mut PerCoordinator,
    finish_reason: &str,
    msg: &Message,
    messages: &mut Vec<Message>,
) -> ReflectOnAssistantOutcome {
    if finish_reason == "tool_calls" {
        return ReflectOnAssistantOutcome::ProceedToExecuteTools;
    }
    match per_coord.after_final_assistant(msg) {
        crate::per_coord::AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
        crate::per_coord::AfterFinalAssistant::RequestPlanRewrite(m) => {
            messages.push(m);
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
        }
    }
}

// --- E：执行 tool_calls（Web）---

pub(crate) struct WebExecuteCtx<'a> {
    pub cfg: &'a AgentConfig,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub out: Option<&'a mpsc::Sender<String>>,
}

pub(crate) struct TuiExecuteCtx<'a> {
    pub cfg: &'a AgentConfig,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub tui_tool_ctx: &'a tool_registry::TuiToolRuntime,
}

#[derive(Clone, Copy)]
pub(crate) enum AgentRunMode<'a> {
    Web { render_to_terminal: bool },
    Tui { tui_tool_ctx: &'a tool_registry::TuiToolRuntime },
}

pub(crate) enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

#[derive(Clone, Copy)]
enum ExecuteDispatchMode<'a> {
    Web,
    Tui(&'a tool_registry::TuiToolRuntime),
}

/// E：执行一批 tool 调用（Web/TUI 共用骨架），写入 tool / 反思 user，并发送 SSE 片段。
async fn per_execute_tools_common(
    tool_calls: &[ToolCall],
    per_coord: &mut PerCoordinator,
    messages: &mut Vec<Message>,
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    out: Option<&mpsc::Sender<String>>,
    dispatch_mode: ExecuteDispatchMode<'_>,
) -> ExecuteToolsBatchOutcome {
    let mut workspace_changed = false;

    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::ToolRunning {
                tool_running: true,
            }))
            .await;
    }

    for tc in tool_calls {
        if let Some(tx) = out
            && tx.is_closed()
        {
            info!("SSE sender closed during tool execution, aborting remaining tools");
            return ExecuteToolsBatchOutcome::AbortedSse;
        }

        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let id = tc.id.clone();
        println!("  [调用工具: {}]", name);

        if let Some(tx) = out
            && let Some(summary) = tools::summarize_tool_call(&name, &args)
        {
            let _ = tx
                .send(encode_message(SsePayload::ToolCall {
                    tool_call: ToolCallSummary {
                        name: name.clone(),
                        summary,
                    },
                }))
                .await;
        }

        let t_tool = Instant::now();
        let (result, reflection_inject) = match dispatch_mode {
            ExecuteDispatchMode::Web => {
                tool_registry::dispatch_tool(
                    ToolRuntime::Web {
                        workspace_changed: &mut workspace_changed,
                    },
                    per_coord,
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    &name,
                    &args,
                    tc,
                )
                .await
            }
            ExecuteDispatchMode::Tui(tui_tool_ctx) => {
                tool_registry::dispatch_tool(
                    ToolRuntime::Tui { ctx: tui_tool_ctx },
                    per_coord,
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    &name,
                    &args,
                    tc,
                )
                .await
            }
        };

        info!(tool = %name, elapsed_ms = t_tool.elapsed().as_millis(), "工具调用完成");

        if let Some(tx) = out {
            let _ = tx
                .send(encode_message(SsePayload::ToolResult {
                    tool_result: ToolResultBody {
                        name: name.clone(),
                        output: result.clone(),
                    },
                }))
                .await;
        }

        PerCoordinator::append_tool_result_and_reflection(messages, id, result, reflection_inject);
    }

    if let Some(tx) = out {
        if matches!(dispatch_mode, ExecuteDispatchMode::Web) && workspace_changed {
            let _ = tx
                .send(encode_message(SsePayload::WorkspaceChanged {
                    workspace_changed: true,
                }))
                .await;
        }
        let _ = tx
            .send(encode_message(SsePayload::ToolRunning {
                tool_running: false,
            }))
            .await;
    }

    ExecuteToolsBatchOutcome::Finished
}

/// E：执行一批 tool 调用，写入 tool / 反思 user，并发送 SSE 片段。
pub(crate) async fn per_execute_tools_web(
    tool_calls: &[ToolCall],
    per_coord: &mut PerCoordinator,
    messages: &mut Vec<Message>,
    ctx: WebExecuteCtx<'_>,
) -> ExecuteToolsBatchOutcome {
    let WebExecuteCtx {
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
    } = ctx;

    per_execute_tools_common(
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
        ExecuteDispatchMode::Web,
    )
    .await
}

/// E（TUI）：执行一批 tool 调用，写入 tool / 反思 user，并发送 SSE 片段。
pub(crate) async fn per_execute_tools_tui(
    tool_calls: &[ToolCall],
    per_coord: &mut PerCoordinator,
    messages: &mut Vec<Message>,
    ctx: TuiExecuteCtx<'_>,
) -> ExecuteToolsBatchOutcome {
    let TuiExecuteCtx {
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
        tui_tool_ctx,
    } = ctx;

    per_execute_tools_common(
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
        ExecuteDispatchMode::Tui(tui_tool_ctx),
    )
    .await
}

/// SSE 发送端已关闭时，应尽快结束外层循环。
pub(crate) fn sse_sender_closed(out: Option<&mpsc::Sender<String>>) -> bool {
    out.is_some_and(|tx| tx.is_closed())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_agent_turn_common(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &AgentConfig,
    tools_defs: &[crate::types::Tool],
    messages: &mut Vec<Message>,
    out: Option<&mpsc::Sender<String>>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    no_stream: bool,
    mode: AgentRunMode<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut per_coord = PerCoordinator::new(cfg.reflection_default_max_rounds);

    'outer: loop {
        if sse_sender_closed(out) {
            info!("SSE sender closed, aborting run_agent_turn loop early");
            break;
        }

        let render_to_terminal = match mode {
            AgentRunMode::Web { render_to_terminal } => render_to_terminal,
            AgentRunMode::Tui { .. } => false,
        };
        let (msg, finish_reason) = per_plan_call_model_retrying(
            client,
            api_key,
            cfg,
            tools_defs,
            messages,
            out,
            render_to_terminal,
            no_stream,
        )
        .await?;
        messages.push(msg.clone());

        match per_reflect_after_assistant(&mut per_coord, &finish_reason, &msg, messages) {
            ReflectOnAssistantOutcome::StopTurn => break,
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => continue 'outer,
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {}
        }

        let tool_calls = msg.tool_calls.as_ref().ok_or("无 tool_calls")?;
        let exec_outcome = match mode {
            AgentRunMode::Web { .. } => {
                per_execute_tools_web(
                    tool_calls,
                    &mut per_coord,
                    messages,
                    WebExecuteCtx {
                        cfg,
                        effective_working_dir,
                        workspace_is_set,
                        out,
                    },
                )
                .await
            }
            AgentRunMode::Tui { tui_tool_ctx } => {
                per_execute_tools_tui(
                    tool_calls,
                    &mut per_coord,
                    messages,
                    TuiExecuteCtx {
                        cfg,
                        effective_working_dir,
                        workspace_is_set,
                        out,
                        tui_tool_ctx,
                    },
                )
                .await
            }
        };
        if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            break;
        }
    }
    Ok(())
}
