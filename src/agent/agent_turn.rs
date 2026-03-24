//! 单轮 Agent 循环的步骤拆分：与「规划–执行–反思」命名对齐的调用边界（P/E/R）。
//!
//! **命名说明**：此处的 **P（Plan）** 指「向模型要本轮输出」——即一次 `llm::complete_chat_retrying`（内部 `llm::api::stream_chat`），由模型产出正文或 `tool_calls`，
//! **不是**独立的符号规划器。**E** 为执行工具；**R** 为终答阶段是否满足结构化规划等（见 `per_coord::after_final_assistant`）。
//!
//! 被 crate 根 [`crate::run_agent_turn`]（Web）与 `runtime::tui`（TUI）共同复用。

use std::path::Path;
use std::sync::Arc;

use crate::config::{AgentConfig, PlannerExecutorMode};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

use log::{debug, info};
use tokio::sync::mpsc;

use super::per_coord::PerCoordinator;
use crate::llm::{complete_chat_retrying, no_tools_chat_request, tool_chat_request};
use crate::sse::{
    SseErrorBody, SsePayload, StagedPlanFinishedBody, StagedPlanStartedBody,
    StagedPlanStepFinishedBody, StagedPlanStepStartedBody, ToolResultBody, encode_message,
};
use crate::tool_registry::{self, ToolRuntime};
use crate::tool_result;
use crate::tools;
use crate::types::{Message, ToolCall, USER_CANCELLED_FINISH_REASON, is_chat_ui_separator};

static STAGED_PLAN_SEQ: AtomicU64 = AtomicU64::new(1);

/// TUI 主循环用 `sync_rx` 拉取对话快照；仅在 TUI 路径传入 `Some`。
async fn tui_push_messages_snapshot(
    sync: Option<&mpsc::Sender<Vec<Message>>>,
    messages: &[Message],
) {
    let Some(tx) = sync else {
        return;
    };
    let _ = tx.send(messages.to_vec()).await;
}

/// TUI（及与 TUI 同形预置占位的会话）在提交前会追加一条 `content` 为空的 `assistant` 供流式写入。
/// Agent 侧若再 `push` 一条同轮助手，会得到 `[…, 空助手, 真助手]`，与 UI 仅一条气泡不对齐，`sync_merge` 后表现为首轮/规划轮输出被「挤没」或错乱。
/// 对**末尾**且仍为空、无 `tool_calls` 的助手占位则**就地替换**。
fn push_assistant_merging_trailing_empty_placeholder(messages: &mut Vec<Message>, msg: Message) {
    if msg.role != "assistant" {
        messages.push(msg);
        return;
    }
    if let Some(last) = messages.last_mut()
        && last.role == "assistant"
        && last.tool_calls.is_none()
        && last
            .content
            .as_deref()
            .map(|s| s.trim())
            .unwrap_or("")
            .is_empty()
    {
        *last = msg;
        return;
    }
    messages.push(msg);
}

/// 规划轮默认 system 追加（可被 `[agent] staged_plan_phase_instruction` 覆盖）。
fn staged_plan_phase_instruction_default() -> String {
    format!(
        "【分阶段规划模式 · 规划轮】请仅根据用户消息做任务拆解，不要调用任何工具，不要执行命令或读写文件。\n\
         在回复正文中必须用 Markdown 代码围栏（语言标记为 json）给出一个合法 JSON 对象，且满足：\n\
         {}\n\
         可辅以简短自然语言说明；后续系统将按 steps 顺序逐步下发执行指令。",
        super::plan_artifact::PLAN_V1_SCHEMA_RULES
    )
}

fn staged_plan_queue_summary_text(
    plan: &super::plan_artifact::AgentReplyPlanV1,
    completed_count: usize,
) -> String {
    let n = plan.steps.len();
    let steps_md =
        super::plan_artifact::format_plan_steps_markdown_for_staged_queue(plan, completed_count);
    let header = format!(
        "{}共 {} 步",
        crate::runtime::plan_section::STAGED_PLAN_SECTION_HEADER,
        n,
    );
    let body = format!("{}\n\n{}", header, steps_md);
    if n > 0 && completed_count >= n {
        format!("[✓] 全部完成\n\n{}", body)
    } else {
        body
    }
}

async fn emit_chat_ui_separator_sse(out: Option<&mpsc::Sender<String>>, short: bool) {
    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::ChatUiSeparator { short }))
            .await;
    }
}

/// 本轮用户消息之后插入与分步结束相同的短分隔线（不进入模型）；若下一条已是分隔线则跳过。
fn insert_separator_after_last_user_for_turn(messages: &mut Vec<Message>) {
    let Some(user_idx) = messages.iter().rposition(|m| m.role == "user") else {
        return;
    };
    if messages.get(user_idx + 1).is_some_and(is_chat_ui_separator) {
        return;
    }
    let sep = Message::chat_ui_separator(true);
    match messages.get(user_idx + 1) {
        Some(m) if m.role == "assistant" => {
            messages.insert(user_idx + 1, sep);
        }
        _ => {
            messages.push(sep);
        }
    }
}

async fn send_staged_plan_notice(
    out: Option<&mpsc::Sender<String>>,
    echo_terminal: bool,
    clear_before: bool,
    text: impl Into<String>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    // CLI（`out: None` 且 `render_to_terminal`）无 SSE，与 TUI 对齐把规划/步骤打到 stdout
    if echo_terminal {
        let _ =
            crate::runtime::terminal_cli_transcript::print_staged_plan_notice(clear_before, &text);
    }
    if let Some(tx) = out {
        let _ = tx
            .send(encode_message(SsePayload::StagedPlanNotice {
                text,
                clear_before,
            }))
            .await;
    }
}

fn next_staged_plan_id() -> String {
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let seq = STAGED_PLAN_SEQ.fetch_add(1, Ordering::Relaxed);
    format!("staged-{ts_ms}-{seq}")
}

async fn send_staged_plan_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    total_steps: usize,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStarted {
            started: StagedPlanStartedBody {
                plan_id: plan_id.to_string(),
                total_steps,
            },
        }))
        .await;
}

async fn send_staged_plan_step_started(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    description: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStepStarted {
            started: StagedPlanStepStartedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                description: description.to_string(),
            },
        }))
        .await;
}

async fn send_staged_plan_step_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    step_id: &str,
    step_index: usize,
    total_steps: usize,
    status: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanStepFinished {
            finished: StagedPlanStepFinishedBody {
                plan_id: plan_id.to_string(),
                step_id: step_id.to_string(),
                step_index,
                total_steps,
                status: status.to_string(),
            },
        }))
        .await;
}

async fn send_staged_plan_finished(
    out: Option<&mpsc::Sender<String>>,
    plan_id: &str,
    total_steps: usize,
    completed_steps: usize,
    status: &str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = tx
        .send(encode_message(SsePayload::StagedPlanFinished {
            finished: StagedPlanFinishedBody {
                plan_id: plan_id.to_string(),
                total_steps,
                completed_steps,
                status: status.to_string(),
            },
        }))
        .await;
}

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
    let filtered: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .cloned()
        .collect();
    let req = tool_chat_request(cfg, &filtered, tools_defs);
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
    pub cfg: &'a Arc<AgentConfig>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// CLI：`render_to_terminal` 且 `out: None` 时为 true，工具结果打印到 stdout（与 TUI 气泡对齐）。
    pub echo_terminal_transcript: bool,
}

pub(crate) struct TuiExecuteCtx<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub tui_tool_ctx: &'a tool_registry::TuiToolRuntime,
}

#[derive(Clone, Copy)]
pub(crate) enum AgentRunMode<'a> {
    Web {
        render_to_terminal: bool,
        web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    },
    Tui {
        tui_tool_ctx: &'a tool_registry::TuiToolRuntime,
    },
}

/// Web/TUI 共用：外层循环与分阶段规划注入共用的一套运行期参数。
pub(crate) struct RunLoopParams<'a> {
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a mut Vec<Message>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub mode: AgentRunMode<'a>,
    pub per_flight: Option<Arc<crate::chat_job_queue::PerTurnFlight>>,
    pub tui_messages_sync: Option<&'a mpsc::Sender<Vec<Message>>>,
}

pub(crate) enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

#[derive(Clone, Copy)]
enum ExecuteDispatchMode<'a> {
    Web(Option<&'a tool_registry::WebToolRuntime>),
    Tui(&'a tool_registry::TuiToolRuntime),
}

/// E：执行一批 tool 调用（Web/TUI 共用骨架），写入 tool / 反思 user，并发送 SSE 片段。
#[allow(clippy::too_many_arguments)] // 工具批处理上下文字段较多，拆结构体收益有限
async fn per_execute_tools_common(
    tool_calls: &[ToolCall],
    per_coord: &mut PerCoordinator,
    messages: &mut Vec<Message>,
    cfg: &Arc<AgentConfig>,
    effective_working_dir: &Path,
    workspace_is_set: bool,
    out: Option<&mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
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
            info!(target: "crabmate", "SSE sender closed during tool execution, aborting remaining tools");
            return ExecuteToolsBatchOutcome::AbortedSse;
        }

        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let id = tc.id.clone();
        // 禁止 println：TUI 下 stdout 与 ratatui 共用终端，会在当前光标（常为输入区）插入乱字。
        info!(target: "crabmate", "调用工具 tool={}", name);
        debug!(
            target: "crabmate",
            "工具调用参数摘要 tool={} args_preview={}",
            name,
            crate::redact::tool_arguments_preview_for_log(&args)
        );

        let tool_summary = tools::summarize_tool_call(&name, &args);

        let t_tool = Instant::now();
        let (result, reflection_inject) = match dispatch_mode {
            ExecuteDispatchMode::Web(web_tool_ctx) => {
                tool_registry::dispatch_tool(
                    ToolRuntime::Web {
                        workspace_changed: &mut workspace_changed,
                        ctx: web_tool_ctx,
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

        info!(
            target: "crabmate",
            "工具调用完成 tool={} elapsed_ms={}",
            name,
            t_tool.elapsed().as_millis()
        );

        if echo_terminal_transcript {
            let _ = crate::runtime::terminal_cli_transcript::print_tool_result_terminal(
                &name,
                &result,
                terminal_tool_display_max_chars,
            );
        }

        if let Some(tx) = out {
            let parsed = tool_result::parse_legacy_output(&name, &result);
            let stdout = if parsed.stdout.is_empty() {
                None
            } else {
                Some(parsed.stdout)
            };
            let stderr = if parsed.stderr.is_empty() {
                None
            } else {
                Some(parsed.stderr)
            };
            let _ = tx
                .send(encode_message(SsePayload::ToolResult {
                    tool_result: ToolResultBody {
                        name: name.clone(),
                        summary: tool_summary,
                        output: result.clone(),
                        ok: Some(parsed.ok),
                        exit_code: parsed.exit_code,
                        error_code: parsed.error_code,
                        stdout,
                        stderr,
                    },
                }))
                .await;
        }

        PerCoordinator::append_tool_result_and_reflection(messages, id, result, reflection_inject);
    }

    if let Some(tx) = out {
        if matches!(dispatch_mode, ExecuteDispatchMode::Web(_)) && workspace_changed {
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
        web_tool_ctx,
        echo_terminal_transcript,
    } = ctx;

    per_execute_tools_common(
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        out,
        echo_terminal_transcript,
        cfg.command_max_output_len,
        ExecuteDispatchMode::Web(web_tool_ctx),
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
        false,
        cfg.command_max_output_len,
        ExecuteDispatchMode::Tui(tui_tool_ctx),
    )
    .await
}

/// SSE 发送端已关闭时，应尽快结束外层循环。
pub(crate) fn sse_sender_closed(out: Option<&mpsc::Sender<String>>) -> bool {
    out.is_some_and(|tx| tx.is_closed())
}

async fn run_agent_outer_loop(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    'outer: loop {
        if sse_sender_closed(p.out) {
            info!(target: "crabmate", "SSE sender closed, aborting run_agent_turn loop early");
            break;
        }
        if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break;
        }

        let render_to_terminal = match p.mode {
            AgentRunMode::Web {
                render_to_terminal, ..
            } => render_to_terminal,
            AgentRunMode::Tui { .. } => false,
        };
        super::context_window::prepare_messages_for_model(
            p.client,
            p.api_key,
            p.cfg.as_ref(),
            p.messages,
        )
        .await?;
        let (msg, finish_reason) = per_plan_call_model_retrying(
            p.client,
            p.api_key,
            p.cfg.as_ref(),
            p.tools_defs,
            p.messages,
            p.out,
            render_to_terminal,
            p.no_stream,
            p.cancel,
        )
        .await?;
        if let Some(f) = p.per_flight.as_ref() {
            f.awaiting_plan_rewrite_model
                .store(false, Ordering::Relaxed);
        }
        debug!(
            target: "crabmate",
            "模型轮次输出 finish_reason={} message_count_before_push={} assistant_preview={}",
            finish_reason,
            p.messages.len(),
            crate::redact::assistant_message_preview_for_log(&msg)
        );
        push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());
        tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;
        if finish_reason == USER_CANCELLED_FINISH_REASON {
            break;
        }

        match per_reflect_after_assistant(per_coord, &finish_reason, &msg, p.messages) {
            ReflectOnAssistantOutcome::StopTurn => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                break;
            }
            ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                    f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
                }
                continue 'outer;
            }
            ReflectOnAssistantOutcome::ProceedToExecuteTools => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
            }
            ReflectOnAssistantOutcome::PlanRewriteExhausted => {
                if let Some(f) = p.per_flight.as_ref() {
                    f.sync_from_per_coord(per_coord);
                }
                if let Some(tx) = p.out {
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
        let echo_terminal_transcript = render_to_terminal && p.out.is_none();
        let exec_outcome = match p.mode {
            AgentRunMode::Web { web_tool_ctx, .. } => {
                per_execute_tools_web(
                    tool_calls,
                    per_coord,
                    p.messages,
                    WebExecuteCtx {
                        cfg: p.cfg,
                        effective_working_dir: p.effective_working_dir,
                        workspace_is_set: p.workspace_is_set,
                        out: p.out,
                        web_tool_ctx,
                        echo_terminal_transcript,
                    },
                )
                .await
            }
            AgentRunMode::Tui { tui_tool_ctx } => {
                per_execute_tools_tui(
                    tool_calls,
                    per_coord,
                    p.messages,
                    TuiExecuteCtx {
                        cfg: p.cfg,
                        effective_working_dir: p.effective_working_dir,
                        workspace_is_set: p.workspace_is_set,
                        out: p.out,
                        tui_tool_ctx,
                    },
                )
                .await
            }
        };
        if matches!(exec_outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            break;
        }
        tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;
        if let Some(f) = p.per_flight.as_ref() {
            f.sync_from_per_coord(per_coord);
        }
    }
    Ok(())
}

async fn run_staged_plan_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = match p.mode {
        AgentRunMode::Web {
            render_to_terminal, ..
        } => render_to_terminal,
        AgentRunMode::Tui { .. } => false,
    };
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    super::context_window::prepare_messages_for_model(
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        p.messages,
    )
    .await?;

    let instr = p.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    let req = no_tools_chat_request(
        p.cfg.as_ref(),
        &build_single_agent_planner_messages(p.messages, plan_system),
    );
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        "分阶段规划轮模型输出",
        "规划轮不应调用工具；请关闭 staged_plan_execution 或重试。",
        "分步注入 user（完整正文，供排障与日志）",
        |body| Message {
            role: "user".to_string(),
            content: Some(body),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
    )
    .await
}

fn build_single_agent_planner_messages(messages: &[Message], plan_system: String) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .cloned()
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

fn build_logical_dual_planner_messages(messages: &[Message], plan_system: String) -> Vec<Message> {
    let mut out: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        // 逻辑双 agent：规划器只看用户/助手自然语言上下文，不看 tool 结果正文，
        // 避免工具细节污染任务拆解。
        .filter(|m| m.role != "tool")
        .filter(|m| {
            if m.role != "assistant" {
                return true;
            }
            m.content
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    out.push(Message::system_only(plan_system));
    out
}

#[allow(clippy::too_many_arguments)] // 分阶段流程抽象后的共享执行骨架
async fn run_staged_plan_with_prepared_request<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    req: crate::types::ChatRequest,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    planning_log_label: &str,
    tool_calls_error_message: &str,
    step_injection_log_label: &str,
    make_step_user_message: F,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
where
    F: Fn(String) -> Message,
{
    let (msg, finish_reason) = complete_chat_retrying(
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        &req,
        p.out,
        render_to_terminal,
        p.no_stream,
        p.cancel,
    )
    .await?;

    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        planning_log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(&msg)
    );

    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return Ok(());
    }

    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        if let Some(tx) = p.out {
            let _ = tx
                .send(encode_message(SsePayload::Error(SseErrorBody {
                    error: tool_calls_error_message.to_string(),
                    code: Some("staged_plan_tool_calls".to_string()),
                })))
                .await;
        }
        return Ok(());
    }

    push_assistant_merging_trailing_empty_placeholder(p.messages, msg.clone());
    tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;

    let content = msg.content.as_deref().unwrap_or("");
    let plan = match super::plan_artifact::parse_agent_reply_plan_v1(content) {
        Ok(plan_v1) => plan_v1,
        Err(_) => {
            if let Some(tx) = p.out {
                let _ = tx
                    .send(encode_message(SsePayload::Error(SseErrorBody {
                        error: "规划轮未解析出合法的 agent_reply_plan v1（需 ```json 围栏或单对象 JSON）。"
                            .to_string(),
                        code: Some("staged_plan_invalid".to_string()),
                    })))
                    .await;
            }
            return Ok(());
        }
    };

    let plan_id = next_staged_plan_id();
    let n = plan.steps.len();
    send_staged_plan_started(p.out, &plan_id, n).await;

    send_staged_plan_notice(
        p.out,
        echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&plan, 0),
    )
    .await;

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    for (i, step) in plan.steps.iter().enumerate() {
        if sse_sender_closed(p.out) || p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            staged_loop_cancelled = true;
            break;
        }
        let step_index = i + 1;
        send_staged_plan_step_started(
            p.out,
            &plan_id,
            step.id.trim(),
            step_index,
            n,
            step.description.trim(),
        )
        .await;

        let summary_hint = if step_index == n && n > 1 {
            format!(
                "\n本步为最后一步，终答中请简要列出本轮全部 {} 个步骤的完成情况（可对每步附简短说明）。",
                n
            )
        } else {
            String::new()
        };
        let body = format!(
            "【分步执行 {}/{}】{}{}\n- id: {}\n- 描述: {}",
            step_index,
            n,
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE,
            summary_hint,
            step.id,
            step.description
        );
        debug!(
            target: "crabmate",
            "{} step={}/{} body_len={} body_preview={}",
            step_injection_log_label,
            i + 1,
            n,
            body.len(),
            crate::redact::preview_chars(&body, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
        );
        if echo_terminal_staged {
            let _ = crate::runtime::terminal_cli_transcript::print_staged_plan_notice(false, &body);
        }
        p.messages.push(make_step_user_message(body));
        tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;
        let run_step = run_agent_outer_loop(p, per_coord).await;
        if let Err(e) = run_step {
            send_staged_plan_step_finished(
                p.out,
                &plan_id,
                step.id.trim(),
                step_index,
                n,
                "failed",
            )
            .await;
            send_staged_plan_finished(p.out, &plan_id, n, completed_steps, "failed").await;
            return Err(e);
        }
        if sse_sender_closed(p.out) || p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            send_staged_plan_step_finished(
                p.out,
                &plan_id,
                step.id.trim(),
                step_index,
                n,
                "cancelled",
            )
            .await;
            staged_loop_cancelled = true;
            break;
        }
        send_staged_plan_step_finished(p.out, &plan_id, step.id.trim(), step_index, n, "ok").await;
        completed_steps = step_index;
        p.messages.push(Message::chat_ui_separator(true));
        // 先发队列摘要 SSE，再 await 全量同步（TUI event forwarder 会在流式间隙立刻下发快照，无需为 sync 通道容量调换顺序）。
        send_staged_plan_notice(
            p.out,
            echo_terminal_staged,
            true,
            staged_plan_queue_summary_text(&plan, step_index),
        )
        .await;
        tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;
        emit_chat_ui_separator_sse(p.out, true).await;
    }
    // 末步成功后循环内已发送含「[✓] 全部完成」的摘要，勿再发一次（否则 CLI/TUI 各重复一条）。
    send_staged_plan_finished(
        p.out,
        &plan_id,
        n,
        completed_steps,
        if staged_loop_cancelled {
            "cancelled"
        } else {
            "ok"
        },
    )
    .await;
    // 仅当循环内未添加过分隔符时再追加：n==0 未进入循环；或取消时 completed_steps==0。
    // 否则末步成功后已在循环内添加，再加会重复两行。
    if n == 0 || (staged_loop_cancelled && completed_steps == 0) {
        p.messages.push(Message::chat_ui_separator(true));
        tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;
        emit_chat_ui_separator_sse(p.out, true).await;
    }
    Ok(())
}

async fn run_logical_dual_agent_then_execute_steps(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let render_to_terminal = match p.mode {
        AgentRunMode::Web {
            render_to_terminal, ..
        } => render_to_terminal,
        AgentRunMode::Tui { .. } => false,
    };
    let echo_terminal_staged = render_to_terminal && p.out.is_none();

    super::context_window::prepare_messages_for_model(
        p.client,
        p.api_key,
        p.cfg.as_ref(),
        p.messages,
    )
    .await?;

    let instr = p.cfg.staged_plan_phase_instruction.trim();
    let plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    let req = no_tools_chat_request(
        p.cfg.as_ref(),
        &build_logical_dual_planner_messages(p.messages, plan_system),
    );
    run_staged_plan_with_prepared_request(
        p,
        per_coord,
        req,
        render_to_terminal,
        echo_terminal_staged,
        "逻辑双agent规划轮输出",
        "规划轮不应调用工具；请检查 planner_executor_mode 配置或重试。",
        "逻辑双agent注入执行器user",
        Message::user_only,
    )
    .await
}

pub(crate) async fn run_agent_turn_common(
    p: &mut RunLoopParams<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    debug!(
        target: "crabmate",
        "run_agent_turn 开始 message_count={} last_user_preview={} staged_plan={} planner_executor_mode={} work_dir={}",
        p.messages.len(),
        crate::redact::last_user_message_preview_for_log(p.messages),
        p.cfg.staged_plan_execution,
        p.cfg.planner_executor_mode.as_str(),
        p.effective_working_dir.display()
    );
    insert_separator_after_last_user_for_turn(p.messages);
    tui_push_messages_snapshot(p.tui_messages_sync, p.messages).await;

    let mut per_coord = PerCoordinator::new(
        p.cfg.reflection_default_max_rounds,
        p.cfg.final_plan_requirement,
        p.cfg.plan_rewrite_max_attempts,
    );

    if p.cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent {
        run_logical_dual_agent_then_execute_steps(p, &mut per_coord).await
    } else if p.cfg.staged_plan_execution {
        run_staged_plan_then_execute_steps(p, &mut per_coord).await
    } else {
        run_agent_outer_loop(p, &mut per_coord).await
    }
}

#[cfg(test)]
mod push_assistant_merge_tests {
    use super::{
        build_logical_dual_planner_messages, build_single_agent_planner_messages,
        push_assistant_merging_trailing_empty_placeholder,
    };
    use crate::types::Message;

    fn empty_assistant() -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(String::new()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn assistant_body(s: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(s.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn replaces_trailing_empty_assistant_placeholder() {
        let mut m = vec![Message::user_only("hi"), empty_assistant()];
        push_assistant_merging_trailing_empty_placeholder(&mut m, assistant_body("plan"));
        assert_eq!(m.len(), 2);
        assert_eq!(m[1].content.as_deref(), Some("plan"));
    }

    #[test]
    fn pushes_when_last_assistant_has_content() {
        let mut m = vec![Message::user_only("hi"), assistant_body("first")];
        push_assistant_merging_trailing_empty_placeholder(&mut m, assistant_body("second"));
        assert_eq!(m.len(), 3);
        assert_eq!(m[2].content.as_deref(), Some("second"));
    }

    #[test]
    fn planner_messages_single_agent_keeps_tool_results() {
        let src = vec![
            Message::system_only("sys"),
            Message::user_only("u1"),
            assistant_body("a1"),
            Message {
                role: "tool".to_string(),
                content: Some("tool out".to_string()),
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_single_agent_planner_messages(&src, "plan sys".to_string());
        assert_eq!(out.len(), 5);
        assert_eq!(out[3].role, "tool");
        assert_eq!(out[4].role, "system");
        assert_eq!(out[4].content.as_deref(), Some("plan sys"));
    }

    #[test]
    fn planner_messages_logical_dual_drops_tool_and_empty_assistant() {
        let src = vec![
            Message::system_only("sys"),
            Message::user_only("u1"),
            assistant_body("a1"),
            empty_assistant(),
            Message {
                role: "tool".to_string(),
                content: Some("tool out".to_string()),
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".to_string()),
            },
        ];
        let out = build_logical_dual_planner_messages(&src, "plan sys".to_string());
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].role, "system");
        assert_eq!(out[1].role, "user");
        assert_eq!(out[2].role, "assistant");
        assert_eq!(out[3].role, "system");
        assert_eq!(out[3].content.as_deref(), Some("plan sys"));
        assert!(!out.iter().any(|m| m.role == "tool"));
    }
}
