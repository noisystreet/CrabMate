//! E 步：执行 tool_calls（SSE/终端、并行只读批、串行带缓存）。

use std::collections::HashSet;

use log::info;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use tokio::sync::mpsc;

static PARALLEL_READONLY_TOOL_BATCH_SEQ: AtomicU64 = AtomicU64::new(1);

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::config::AgentConfig;
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::sse::{SsePayload, encode_message};
use crate::tool_registry;
use crate::tool_result::ToolEnvelopeContext;
use crate::types::{Message, Tool, ToolCall};
use crate::workspace::changelist::WorkspaceChangelist;

mod emit;
mod parallel_readonly;
mod run_command_guard;
mod serial;
use emit::{
    emit_sse_tool_running, emit_timeline_log_sse, emit_tool_call_summary_sse,
    emit_tool_result_sse_and_append,
};
use parallel_readonly::execute_tools_parallel;
use serial::execute_tools_serial;

/// 本模块 `tracing` / `log` 的 `target`，便于 `RUST_LOG=crabmate::execute_tools` 过滤。
const LOG_TARGET: &str = "crabmate::execute_tools";

fn trace_parallel_tool_child_span(
    tracing_turn: Option<&Arc<crate::observability::TracingChatTurn>>,
    tool_call_id: &str,
) -> tracing::Span {
    match tracing_turn {
        Some(t) => {
            let tool_call_id_label = t.record_tool_call_id_for_log(tool_call_id);
            tracing::span!(
                parent: t.span.id(),
                tracing::Level::INFO,
                "parallel_tool",
                tool_call_id = %tool_call_id_label,
            )
        }
        None => tracing::Span::none(),
    }
}

pub(crate) struct WebExecuteCtx<'a> {
    pub cfg: &'a Arc<AgentConfig>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    /// 单轮 `read_file` 缓存；`None` 表示关闭。
    pub read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub tool_running_hook: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    pub clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 终端 CLI：`run_command` 非白名单时 stdin 审批；`None` 时与历史一致（非白名单则无法执行）。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    /// CLI：`render_to_terminal` 且 `out: None` 时为 true，工具结果打印到 stdout。
    pub echo_terminal_transcript: bool,
    /// MCP stdio 会话；`None` 时 `mcp__*` 工具会报错。
    pub mcp_turn: Option<&'a crate::mcp::McpTurnHandle>,
    pub workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    /// 整请求 Chrome trace；与 `workflow_execute` 合并写 `turn-*.json`。
    pub request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    /// 分阶段规划当前步子代理约束；与 `RunLoopTurnState::turn_planner_hints.step_executor_constraint` 同步。
    pub step_executor_constraint: Option<PlanStepExecutorKind>,
    /// 本会话完整工具定义（未按步收窄）；用于子代理越权提示中的允许工具名列表。
    pub tools_defs_full: &'a [Tool],
    /// 多角色工具白名单；`None` 不限制。
    pub turn_allow: Option<&'a HashSet<String>>,
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    pub long_term_memory_scope_id: Option<String>,
    pub tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    /// Web 审计；与写工具日志配套。
    pub request_audit: Option<Arc<crate::web::audit::WebRequestAudit>>,
    /// 与 [`crate::RunAgentTurnParams::process_handles`] 同源。
    pub tool_outcome_recorder: Arc<crate::tool_stats::ToolOutcomeRecorder>,
    /// 与 `process_handles.handler_lookup` 同源（随 `RunLoopCtx` 注入，避免在批处理中再借 `process_handles`）。
    pub handler_lookup: crate::tool_registry::HandlerLookupTable,
    pub sync_default_sandbox_backend: Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
    /// 与 [`crate::process_handles::ProcessHandles::readonly_tool_ttl_cache`] 同源。
    pub readonly_tool_ttl_cache: Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    /// 无 HTTP SSE 时镜像控制面（与 Web `SsePayload` 对齐）；Web 为 `None`。
    pub sse_control_mirror: Option<crate::sse::SseControlMirror>,
}

pub(crate) enum ExecuteToolsBatchOutcome {
    /// 本批工具跑完，继续外层循环
    Finished,
    /// SSE 在工具执行中断开
    AbortedSse,
}

/// 单工具：SSE / 终端回显 + 追加 `tool` 与可选反思 `user`（与串行路径一致）的入参。
pub(super) struct EmitToolResultParams<'a> {
    cfg: &'a Arc<AgentConfig>,
    tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a mpsc::Sender<String>>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
    /// 无 SSE 时（如 `crabmate tui`）：仍通知澄清问卷控制面，与 Web SSE 语义对齐。
    clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    name: &'a str,
    args: &'a str,
    id: &'a str,
    result: String,
    reflection_inject: Option<serde_json::Value>,
    envelope_ctx: Option<ToolEnvelopeContext<'a>>,
}

/// SSE 发送端已关闭（与外层 `run_agent_turn` 早退判断一致）。
pub(crate) fn sse_sender_closed(out: Option<&mpsc::Sender<String>>) -> bool {
    out.is_some_and(|tx| tx.is_closed())
}

/// 工具批处理中发现 SSE 已断开：记日志、尽力下发「工具轮结束」，返回 `true` 时应中止批处理。
async fn abort_tool_batch_if_sse_closed(
    out: Option<&mpsc::Sender<String>>,
    reason: &'static str,
) -> bool {
    if !sse_sender_closed(out) {
        return false;
    }
    info!(target: LOG_TARGET, "{reason}");
    emit_sse_tool_running(
        out,
        false,
        "execute_tools::abort_tool_batch tool_running false",
    )
    .await;
    true
}

/// 统计并行只读批次中去重后的唯一 `(name, args)` 数。
pub(crate) fn dedup_readonly_tool_calls_count(tool_calls: &[ToolCall]) -> usize {
    let mut seen: HashSet<(&str, &str)> = HashSet::with_capacity(tool_calls.len());
    for tc in tool_calls {
        seen.insert((tc.function.name.as_str(), tc.function.arguments.as_str()));
    }
    seen.len()
}

/// E：执行一批 tool 调用（Web/CLI 共用骨架），写入 tool / 反思 user，并发送 SSE 片段。
///
/// 同名同参数的只读工具在同一批次内去重：并行路径只执行唯一实例后映射回各 `tool_call_id`；
/// 串行路径维护本批次只读缓存，遇写操作时清空（写操作可能改变文件系统状态，使先前读取结果失效）。
struct ExecuteToolsCommonCtx<'a> {
    tool_calls: &'a [ToolCall],
    per_coord: &'a mut PerCoordinator,
    messages: &'a mut Vec<Message>,
    cfg: &'a Arc<AgentConfig>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    read_file_turn_cache: Option<Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    workspace_changelist: Option<&'a Arc<WorkspaceChangelist>>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    mcp_turn: Option<&'a crate::mcp::McpTurnHandle>,
    request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    step_executor_constraint: Option<PlanStepExecutorKind>,
    tools_defs_full: &'a [Tool],
    turn_allow: Option<&'a HashSet<String>>,
    long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    tracing_chat_turn: Option<Arc<crate::observability::TracingChatTurn>>,
    request_audit: Option<Arc<crate::web::audit::WebRequestAudit>>,
    tool_outcome_recorder: Arc<crate::tool_stats::ToolOutcomeRecorder>,
    handler_lookup: crate::tool_registry::HandlerLookupTable,
    sync_default_sandbox_backend: Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
    readonly_tool_ttl_cache: Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    tool_running_hook: Option<Arc<dyn Fn(bool) + Send + Sync>>,
    clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
}

fn notify_cli_tool_running_hook(
    out: Option<&mpsc::Sender<String>>,
    hook: Option<&Arc<dyn Fn(bool) + Send + Sync>>,
    running: bool,
) {
    if out.is_some() {
        return;
    }
    if let Some(h) = hook {
        h(running);
    }
}

async fn per_execute_tools_common(ctx: ExecuteToolsCommonCtx<'_>) -> ExecuteToolsBatchOutcome {
    let tool_running_hook = ctx.tool_running_hook.clone();
    let out = ctx.out;
    let force_serial = std::env::var("CM_REPLAY_FORCE_SERIAL")
        .ok()
        .map(|v| v.trim().to_ascii_lowercase())
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"));

    emit_sse_tool_running(out, true, "execute_tools::batch tool_running true").await;
    notify_cli_tool_running_hook(out, tool_running_hook.as_ref(), true);

    let workspace_changed = if !force_serial
        && ctx.workspace_is_set
        && crate::agent_role_turn::tool_calls_allow_parallel_for_role(
            &ctx.handler_lookup,
            ctx.cfg.as_ref(),
            ctx.tool_calls,
            ctx.turn_allow,
        ) {
        crate::turn_replay_dump::append_decision_point_event_if_configured(
            "tool_execution",
            "tool_batch_execution_mode",
            "parallel_readonly_batch",
            "当前批次满足只读并行条件，采用并行只读批执行以提升吞吐",
            serde_json::json!({
                "force_serial": force_serial,
                "workspace_is_set": ctx.workspace_is_set,
                "tool_call_count": ctx.tool_calls.len(),
            }),
            "current_tool_batch",
            None,
        );
        let outcome = execute_tools_parallel(ctx).await;
        if matches!(outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            emit_sse_tool_running(
                out,
                false,
                "execute_tools::batch aborted_after_parallel tool_running false",
            )
            .await;
            notify_cli_tool_running_hook(out, tool_running_hook.as_ref(), false);
            return outcome;
        }
        false
    } else {
        crate::turn_replay_dump::append_decision_point_event_if_configured(
            "tool_execution",
            "tool_batch_execution_mode",
            "serial",
            if force_serial {
                "环境变量强制串行执行，关闭并行只读批"
            } else {
                "当前批次不满足并行条件，回退串行执行"
            },
            serde_json::json!({
                "force_serial": force_serial,
                "workspace_is_set": ctx.workspace_is_set,
                "tool_call_count": ctx.tool_calls.len(),
            }),
            "current_tool_batch",
            None,
        );
        if force_serial {
            log::info!(
                target: LOG_TARGET,
                "CM_REPLAY_FORCE_SERIAL enabled: force serial tool execution"
            );
            crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
                "tool_batch_mode",
                "force_serial",
                Some(&serde_json::json!({
                    "source": "CM_REPLAY_FORCE_SERIAL",
                    "parallel_disabled": true
                })),
            );
        }
        let mut workspace_changed = false;
        let outcome = execute_tools_serial(ctx, &mut workspace_changed).await;
        if matches!(outcome, ExecuteToolsBatchOutcome::AbortedSse) {
            emit_sse_tool_running(
                out,
                false,
                "execute_tools::batch aborted_after_serial tool_running false",
            )
            .await;
            notify_cli_tool_running_hook(out, tool_running_hook.as_ref(), false);
            return outcome;
        }
        workspace_changed
    };

    if let Some(tx) = out
        && workspace_changed
    {
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::WorkspaceChanged {
                workspace_changed: true,
            }),
            "execute_tools::batch workspace_changed",
        )
        .await;
    }
    emit_sse_tool_running(out, false, "execute_tools::batch tool_running false").await;
    notify_cli_tool_running_hook(out, tool_running_hook.as_ref(), false);

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
        read_file_turn_cache,
        out,
        tool_running_hook,
        clarification_questionnaire_hook,
        web_tool_ctx,
        cli_tool_ctx,
        echo_terminal_transcript,
        mcp_turn,
        workspace_changelist,
        request_chrome_trace,
        step_executor_constraint,
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
        request_audit,
        tool_outcome_recorder,
        handler_lookup,
        sync_default_sandbox_backend,
        readonly_tool_ttl_cache,
        sse_control_mirror,
    } = ctx;

    let _tool_trace = request_chrome_trace
        .as_ref()
        .map(|t| t.enter_section("agent.tools_batch"));

    per_execute_tools_common(ExecuteToolsCommonCtx {
        tool_calls,
        per_coord,
        messages,
        cfg,
        effective_working_dir,
        workspace_is_set,
        read_file_turn_cache,
        workspace_changelist,
        out,
        tool_running_hook,
        clarification_questionnaire_hook,
        echo_terminal_transcript,
        terminal_tool_display_max_chars: cfg.command_exec.command_max_output_len,
        tool_result_envelope_v1: cfg.tool_transcript.tool_result_envelope_v1,
        web_tool_ctx,
        cli_tool_ctx,
        mcp_turn,
        request_chrome_trace,
        step_executor_constraint,
        tools_defs_full,
        turn_allow,
        long_term_memory,
        long_term_memory_scope_id,
        tracing_chat_turn,
        request_audit,
        tool_outcome_recorder,
        handler_lookup,
        sync_default_sandbox_backend,
        readonly_tool_ttl_cache,
        sse_control_mirror,
    })
    .await
}
