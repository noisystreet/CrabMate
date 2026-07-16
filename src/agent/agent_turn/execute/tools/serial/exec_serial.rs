//! 串行工具批执行主循环（从 [`super::`] 拆出以降低 `serial/mod.rs` 行数）。

use std::collections::HashMap;
use std::time::Instant;

use log::info;

use crate::agent::agent_turn::execute::ToolExecutionHost;
use crate::agent::agent_turn::execute::tool_execution_host::CrabmateToolExecutionHost;
use crate::tool_registry::{self, ToolRuntime};
use crate::types::ToolCall;

use std::sync::Arc;

use super::super::{
    ExecuteToolsBatchOutcome, ExecuteToolsCommonCtx, abort_tool_batch_if_sse_closed,
};
use super::after_dispatch::{
    serial_bookkeep_run_command_failure,
    serial_clear_run_command_failures_after_workspace_mutation,
    serial_log_web_audit_write_tool_if_needed, serial_maybe_invalidate_codebase_semantic_index,
    serial_tool_iteration_sse_preface,
};
use super::emit::{
    emit_serial_tool_result, serial_bookkeep_readonly_tool_ttl_cache_after_tool,
    serial_emit_early_without_dispatch,
};
use super::{
    SerialEmitEarlyWithoutDispatchParams, SerialEmitToolResultParams,
    SerialToolIterationSsePreface, SerialTtlAfterDispatchParams,
};

pub(super) async fn execute_tools_serial_impl(
    ctx: ExecuteToolsCommonCtx<'_>,
    workspace_changed: &mut bool,
) -> ExecuteToolsBatchOutcome {
    let mut loop_state = SerialToolLoopState::from_ctx(ctx, workspace_changed);
    let mut readonly_cache: HashMap<(String, String), String> = HashMap::new();
    for tc in loop_state.tool_calls {
        if serial_execute_one_tool_call(&mut loop_state, &mut readonly_cache, tc).await {
            return ExecuteToolsBatchOutcome::AbortedSse;
        }
    }
    ExecuteToolsBatchOutcome::Finished
}

struct SerialToolLoopState<'a> {
    tool_calls: &'a [ToolCall],
    per_coord: &'a mut crate::agent::per_coord::PerCoordinator,
    messages: &'a mut Vec<crate::types::Message>,
    cfg: &'a std::sync::Arc<crate::config::AgentConfig>,
    effective_working_dir: &'a std::path::Path,
    workspace_is_set: bool,
    read_file_turn_cache: Option<std::sync::Arc<crate::read_file_turn_cache::ReadFileTurnCache>>,
    workspace_changelist:
        Option<&'a std::sync::Arc<crate::workspace::changelist::WorkspaceChangelist>>,
    out: Option<&'a tokio::sync::mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    web_tool_ctx: Option<&'a crate::tool_registry::WebToolRuntime>,
    cli_tool_ctx: Option<&'a crate::tool_registry::CliToolRuntime>,
    mcp_turn: Option<&'a crate::mcp::McpTurnHandle>,
    request_chrome_trace: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    step_executor_constraint: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    tools_defs_full: &'a [crate::types::Tool],
    turn_allow: Option<&'a std::collections::HashSet<String>>,
    long_term_memory:
        Option<std::sync::Arc<crate::memory::long_term_memory::LongTermMemoryRuntime>>,
    long_term_memory_scope_id: Option<String>,
    tracing_chat_turn: Option<std::sync::Arc<crate::observability::TracingChatTurn>>,
    request_audit: Option<std::sync::Arc<crate::WebRequestAudit>>,
    tool_outcome_recorder: std::sync::Arc<crate::tool_stats::ToolOutcomeRecorder>,
    handler_lookup: crate::tool_registry::HandlerLookupTable,
    sync_default_sandbox_backend:
        std::sync::Arc<dyn crate::tool_sandbox::SyncDefaultSandboxBackend>,
    readonly_tool_ttl_cache: std::sync::Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    clarification_questionnaire_hook:
        Option<std::sync::Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
    sse_encoder: Arc<dyn crate::sse::SseEncoder>,
    workspace_changed: &'a mut bool,
}

impl<'a> SerialToolLoopState<'a> {
    fn from_ctx(ctx: ExecuteToolsCommonCtx<'a>, workspace_changed: &'a mut bool) -> Self {
        let ExecuteToolsCommonCtx {
            tool_calls,
            per_coord,
            messages,
            cfg,
            effective_working_dir,
            workspace_is_set,
            read_file_turn_cache,
            workspace_changelist,
            out,
            tool_running_hook: _,
            clarification_questionnaire_hook,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
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
            sse_encoder,
        } = ctx;
        Self {
            tool_calls,
            per_coord,
            messages,
            cfg,
            effective_working_dir,
            workspace_is_set,
            read_file_turn_cache,
            workspace_changelist,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
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
            clarification_questionnaire_hook,
            sse_control_mirror,
            sse_encoder,
            workspace_changed,
        }
    }
}

/// 返回 `true` 表示应中止整批工具执行（SSE 已关闭）。
async fn serial_execute_one_tool_call(
    st: &mut SerialToolLoopState<'_>,
    readonly_cache: &mut HashMap<(String, String), String>,
    tc: &ToolCall,
) -> bool {
    if abort_tool_batch_if_sse_closed(
        st.out,
        "SSE sender closed during tool execution, aborting remaining tools",
        st.sse_encoder.as_ref(),
    )
    .await
    {
        return true;
    }

    let name = tc.function.name.clone();
    let args = tc.function.arguments.clone();
    let id = tc.id.clone();
    let sse_mirror_for_emit = st.sse_control_mirror.clone();

    serial_tool_iteration_sse_preface(SerialToolIterationSsePreface {
        out: st.out,
        sse_mirror: sse_mirror_for_emit.as_ref(),
        cfg: st.cfg,
        tracing_chat_turn: st.tracing_chat_turn.as_ref(),
        id: id.as_str(),
        name: &name,
        args: &args,
        messages: st.messages,
        encoder: st.sse_encoder.as_ref(),
    })
    .await;

    if serial_emit_early_without_dispatch(SerialEmitEarlyWithoutDispatchParams {
        messages: st.messages,
        per_coord: st.per_coord,
        cfg: st.cfg,
        tool_outcome_recorder: &st.tool_outcome_recorder,
        out: st.out,
        sse_control_mirror: sse_mirror_for_emit.clone(),
        clarification_questionnaire_hook: st.clarification_questionnaire_hook.clone(),
        echo_terminal_transcript: st.echo_terminal_transcript,
        terminal_tool_display_max_chars: st.terminal_tool_display_max_chars,
        tool_result_envelope_v1: st.tool_result_envelope_v1,
        effective_working_dir: st.effective_working_dir,
        name: name.as_str(),
        args: args.as_str(),
        id: id.as_str(),
        step_executor_constraint: st.step_executor_constraint,
        tools_defs_full: st.tools_defs_full,
        turn_allow: st.turn_allow,
        readonly_cache,
        readonly_tool_ttl_cache: &st.readonly_tool_ttl_cache,
        encoder: st.sse_encoder.as_ref(),
    })
    .await
    {
        return false;
    }

    let is_readonly = tool_registry::is_readonly_tool(st.cfg.as_ref(), name.as_str());
    let cache_key = (name.clone(), args.clone());
    let t_tool = Instant::now();
    let runtime = if let Some(cctx) = st.cli_tool_ctx {
        ToolRuntime::Cli {
            workspace_changed: st.workspace_changed,
            ctx: cctx,
        }
    } else {
        ToolRuntime::Web {
            workspace_changed: st.workspace_changed,
            ctx: st.web_tool_ctx,
        }
    };
    let (result, reflection_inject) = {
        let mut host = CrabmateToolExecutionHost {
            per_coord: st.per_coord,
            request_chrome_trace: st.request_chrome_trace.clone(),
        };
        host.dispatch_tool_call(
            name.as_str(),
            tool_registry::DispatchToolParams {
                runtime,
                cfg: st.cfg,
                effective_working_dir: st.effective_working_dir,
                workspace_is_set: st.workspace_is_set,
                name: &name,
                args: &args,
                sse_out_tx: st.out,
                sse_control_mirror: sse_mirror_for_emit.as_ref(),
                tc,
                read_file_turn_cache: st.read_file_turn_cache.clone(),
                workspace_changelist: st.workspace_changelist.cloned(),
                mcp_turn: st.mcp_turn,
                turn_allow: st.turn_allow,
                long_term_memory: st.long_term_memory.clone(),
                long_term_memory_scope_id: st.long_term_memory_scope_id.clone(),
                handler_lookup: &st.handler_lookup,
                sync_default_sandbox_backend: &st.sync_default_sandbox_backend,
            },
        )
        .await
    };

    info!(
        target: super::super::LOG_TARGET,
        "工具调用完成 tool={} args_preview={} elapsed_ms={}",
        name,
        crate::redact::tool_arguments_preview_for_log(&args),
        t_tool.elapsed().as_millis()
    );

    serial_log_web_audit_write_tool_if_needed(
        st.cfg.as_ref(),
        is_readonly,
        st.request_audit.as_ref(),
        st.tracing_chat_turn.as_ref(),
        st.long_term_memory_scope_id.as_deref(),
        name.as_str(),
        args.as_str(),
    );

    serial_bookkeep_run_command_failure(
        st.per_coord,
        name.as_str(),
        args.as_str(),
        result.as_str(),
    );

    serial_clear_run_command_failures_after_workspace_mutation(
        st.per_coord,
        st.cfg.as_ref(),
        is_readonly,
        *st.workspace_changed,
        name.as_str(),
        result.as_str(),
    );

    serial_maybe_invalidate_codebase_semantic_index(
        st.cfg,
        st.effective_working_dir,
        st.workspace_changed,
        is_readonly,
        name.as_str(),
        args.as_str(),
        result.as_str(),
    );

    serial_bookkeep_readonly_tool_ttl_cache_after_tool(SerialTtlAfterDispatchParams {
        cfg: st.cfg.as_ref(),
        effective_working_dir: st.effective_working_dir,
        readonly_tool_ttl_cache: &st.readonly_tool_ttl_cache,
        name: name.as_str(),
        args: args.as_str(),
        result: result.as_str(),
        workspace_changed: *st.workspace_changed,
    });

    if (!is_readonly || *st.workspace_changed)
        && let Some(c) = st.read_file_turn_cache.as_ref()
    {
        c.clear();
    }

    if is_readonly {
        readonly_cache.insert(cache_key, result.clone());
    } else {
        readonly_cache.clear();
    }

    emit_serial_tool_result(SerialEmitToolResultParams {
        messages: st.messages,
        per_coord: st.per_coord,
        cfg: st.cfg,
        tool_outcome_recorder: &st.tool_outcome_recorder,
        out: st.out,
        sse_control_mirror: sse_mirror_for_emit,
        clarification_questionnaire_hook: st.clarification_questionnaire_hook.clone(),
        echo_terminal_transcript: st.echo_terminal_transcript,
        terminal_tool_display_max_chars: st.terminal_tool_display_max_chars,
        tool_result_envelope_v1: st.tool_result_envelope_v1,
        name: name.as_str(),
        args: args.as_str(),
        id: id.as_str(),
        result,
        reflection_inject,
        encoder: st.sse_encoder.as_ref(),
    })
    .await;
    false
}
