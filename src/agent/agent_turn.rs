//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web）与 `runtime::tui`（TUI）共同复用。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tokio::sync::mpsc;
use tracing::info;

use super::per_coord::PerCoordinator;
use crate::config::AgentConfig;
use crate::llm::{complete_chat_retrying, tool_chat_request};
use crate::sse::{SseErrorBody, SsePayload, ToolCallSummary, ToolResultBody, encode_message};
use crate::tool_registry::{self, ToolRuntime};
use crate::tool_result::ToolResult as StructuredToolResult;
use crate::tools;
use crate::types::{Message, ToolCall, USER_CANCELLED_FINISH_REASON};

// --- P：向模型要本轮输出（含重试）---

/// P：构造请求并调用模型（`no_stream` 为 true 时走 `stream: false`），**不**修改 `messages`。
#[allow(clippy::too_many_arguments)] // Web/TUI 共用入口，参数扁平便于各调用点传参
pub(crate) async fn per_plan_call_model_retrying(
    client: &reqwest::Client,
    api_key: &str,
    cfg: &AgentConfig,
    tools_defs: &[crate::types::Tool],
    messages: &[Message],
    out: Option<&mpsc::Sender<String>>,
    render_to_terminal: bool,
    no_stream: bool,
    cancel: Option<&AtomicBool>,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let req = tool_chat_request(cfg, messages, tools_defs);
    complete_chat_retrying(
        client,
        api_key,
        cfg,
        &req,
        out,
        render_to_terminal,
        no_stream,
        cancel,
    )
    .await
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
    /// 规划重写次数用尽（已尝试发 SSE 错误码 `plan_rewrite_exhausted`）
    PlanRewriteExhausted,
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
    match per_coord.after_final_assistant(msg, messages.as_slice()) {
        super::per_coord::AfterFinalAssistant::StopTurn => ReflectOnAssistantOutcome::StopTurn,
        super::per_coord::AfterFinalAssistant::RequestPlanRewrite(m) => {
            messages.push(m);
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite
        }
        super::per_coord::AfterFinalAssistant::StopTurnPlanRewriteExhausted => {
            ReflectOnAssistantOutcome::PlanRewriteExhausted
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
    Web {
        render_to_terminal: bool,
    },
    Tui {
        tui_tool_ctx: &'a tool_registry::TuiToolRuntime,
    },
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
#[allow(clippy::too_many_arguments)] // 工具批处理上下文字段较多，拆结构体收益有限
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
        // 禁止 println：TUI 下 stdout 与 ratatui 共用终端，会在当前光标（常为输入区）插入乱字。
        info!(tool = %name, "调用工具");

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
            let structured = StructuredToolResult::from_legacy_output(&name, result.clone());
            let stdout = if structured.stdout.is_empty() {
                None
            } else {
                Some(structured.stdout)
            };
            let stderr = if structured.stderr.is_empty() {
                None
            } else {
                Some(structured.stderr)
            };
            let _ = tx
                .send(encode_message(SsePayload::ToolResult {
                    tool_result: ToolResultBody {
                        name: name.clone(),
                        output: result.clone(),
                        ok: Some(structured.ok),
                        exit_code: structured.exit_code,
                        error_code: structured.error_code,
                        stdout,
                        stderr,
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
    cancel: Option<&AtomicBool>,
    mode: AgentRunMode<'_>,
    per_flight: Option<Arc<crate::chat_job_queue::PerTurnFlight>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut per_coord = PerCoordinator::new(
        cfg.reflection_default_max_rounds,
        cfg.final_plan_requirement,
        cfg.plan_rewrite_max_attempts,
    );

    'outer: loop {
        if sse_sender_closed(out) {
            info!("SSE sender closed, aborting run_agent_turn loop early");
            break;
        }
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break;
        }

        let render_to_terminal = match mode {
            AgentRunMode::Web { render_to_terminal } => render_to_terminal,
            AgentRunMode::Tui { .. } => false,
        };
        super::context_window::prepare_messages_for_model(client, api_key, cfg, messages).await?;
        let (msg, finish_reason) = per_plan_call_model_retrying(
            client,
            api_key,
            cfg,
            tools_defs,
            messages,
            out,
            render_to_terminal,
            no_stream,
            cancel,
        )
        .await?;
        if let Some(f) = per_flight.as_ref() {
            f.awaiting_plan_rewrite_model
                .store(false, Ordering::Relaxed);
        }
        messages.push(msg.clone());
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            break;
        }

        match per_reflect_after_assistant(&mut per_coord, &finish_reason, &msg, messages) {
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = per_flight.as_ref() {
                    f.sync_from_per_coord(&per_coord);
                }
                break;
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = per_flight.as_ref() {
                    f.sync_from_per_coord(&per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                continue 'outer;
            }
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                if let Some(f) = per_flight.as_ref() {
                    f.sync_from_per_coord(&per_coord);
                }
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted => {
                if let Some(f) = per_flight.as_ref() {
                    f.sync_from_per_coord(&per_coord);
                }
                if let Some(tx) = out {
                    let _ = tx
                        .send(encode_message(SsePayload::Error(SseErrorBody {
                            error: PerCoordinator::plan_rewrite_exhausted_sse_message().to_string(),
                            code: Some("plan_rewrite_exhausted".to_string()),
                        })))
                        .await;
                }
                break;
            }
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
        if let Some(f) = per_flight.as_ref() {
            f.sync_from_per_coord(&per_coord);
        }
    }
    Ok(())
}
