use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use log::info;
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::agent::workflow_tool_dispatch;
use crate::tool_registry::{self, HandlerId, ToolRuntime};

use super::{ExecuteToolsBatchOutcome, ExecuteToolsCommonCtx, abort_tool_batch_if_sse_closed};

mod after_dispatch;
mod emit;

use after_dispatch::{
    serial_bookkeep_run_command_failure, serial_log_web_audit_write_tool_if_needed,
    serial_maybe_invalidate_codebase_semantic_index, serial_tool_iteration_sse_preface,
};
use emit::{
    emit_serial_tool_result, serial_bookkeep_readonly_tool_ttl_cache_after_tool,
    serial_emit_early_without_dispatch,
};

/// 串行工具路径：统一构造 `ToolEnvelopeContext` 并下发 SSE / 追加消息。
pub(super) struct SerialEmitToolResultParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) result: String,
    pub(super) reflection_inject: Option<serde_json::Value>,
}

pub(super) struct SerialTtlRunCommandEarlyHitParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) effective_working_dir: &'a Path,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) readonly_tool_ttl_cache:
        &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
}

pub(super) struct SerialEarlyToolPolicyDenyParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) step_executor_constraint: Option<PlanStepExecutorKind>,
    pub(super) tools_defs_full: &'a [crate::types::Tool],
    pub(super) turn_allow: Option<&'a HashSet<String>>,
}

pub(super) struct SerialTtlAfterDispatchParams<'a> {
    pub(super) cfg: &'a crate::config::AgentConfig,
    pub(super) effective_working_dir: &'a Path,
    pub(super) readonly_tool_ttl_cache:
        &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) result: &'a str,
    pub(super) workspace_changed: bool,
}

/// 预检 / 策略拒绝 / `run_command` 短路 / 只读缓存命中：若已下发工具结果则返回 `true`（外层应 `continue`）。
pub(super) struct SerialEmitEarlyWithoutDispatchParams<'a> {
    pub(super) messages: &'a mut Vec<crate::types::Message>,
    pub(super) per_coord: &'a mut PerCoordinator,
    pub(super) cfg: &'a Arc<crate::config::AgentConfig>,
    pub(super) tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_control_mirror: Option<crate::sse::SseControlMirror>,
    pub(super) clarification_questionnaire_hook:
        Option<Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    pub(super) echo_terminal_transcript: bool,
    pub(super) terminal_tool_display_max_chars: usize,
    pub(super) tool_result_envelope_v1: bool,
    pub(super) effective_working_dir: &'a Path,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) id: &'a str,
    pub(super) step_executor_constraint: Option<PlanStepExecutorKind>,
    pub(super) tools_defs_full: &'a [crate::types::Tool],
    pub(super) turn_allow: Option<&'a HashSet<String>>,
    pub(super) readonly_cache: &'a mut HashMap<(String, String), String>,
    pub(super) readonly_tool_ttl_cache:
        &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
}

/// 每轮工具迭代开头：记录 tracing、下发 `tool_call` / `timeline`、打调用日志（从 [`execute_tools_serial`] 拆出以降低 nloc）。
pub(super) struct SerialToolIterationSsePreface<'a> {
    pub(super) out: Option<&'a mpsc::Sender<String>>,
    pub(super) sse_mirror: Option<&'a crate::sse::SseControlMirror>,
    pub(super) cfg: &'a std::sync::Arc<crate::config::AgentConfig>,
    pub(super) tracing_chat_turn: Option<&'a std::sync::Arc<crate::observability::TracingChatTurn>>,
    pub(super) id: &'a str,
    pub(super) name: &'a str,
    pub(super) args: &'a str,
    pub(super) messages: &'a [crate::types::Message],
}

/// 串行路径：`dispatch_tool`、只读结果缓存、写操作后清缓存。
pub(super) async fn execute_tools_serial(
    ctx: ExecuteToolsCommonCtx<'_>,
    workspace_changed: &mut bool,
) -> ExecuteToolsBatchOutcome {
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
        mcp_session,
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

    let sse_mirror_for_emit = sse_control_mirror.clone();

    let mut readonly_cache: HashMap<(String, String), String> = HashMap::new();
    for tc in tool_calls {
        if abort_tool_batch_if_sse_closed(
            out,
            "SSE sender closed during tool execution, aborting remaining tools",
        )
        .await
        {
            return ExecuteToolsBatchOutcome::AbortedSse;
        }

        let name = tc.function.name.clone();
        let args = tc.function.arguments.clone();
        let id = tc.id.clone();
        serial_tool_iteration_sse_preface(SerialToolIterationSsePreface {
            out,
            sse_mirror: sse_mirror_for_emit.as_ref(),
            cfg,
            tracing_chat_turn: tracing_chat_turn.as_ref(),
            id: id.as_str(),
            name: &name,
            args: &args,
            messages,
        })
        .await;

        if serial_emit_early_without_dispatch(SerialEmitEarlyWithoutDispatchParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder: &tool_outcome_recorder,
            out,
            sse_control_mirror: sse_mirror_for_emit.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            effective_working_dir,
            name: name.as_str(),
            args: args.as_str(),
            id: id.as_str(),
            step_executor_constraint,
            tools_defs_full,
            turn_allow,
            readonly_cache: &mut readonly_cache,
            readonly_tool_ttl_cache: &readonly_tool_ttl_cache,
        })
        .await
        {
            continue;
        }

        let is_readonly = tool_registry::is_readonly_tool(cfg.as_ref(), name.as_str());
        let cache_key = (name.clone(), args.clone());

        let t_tool = Instant::now();
        let runtime = if let Some(cctx) = cli_tool_ctx {
            ToolRuntime::Cli {
                workspace_changed,
                ctx: cctx,
            }
        } else {
            ToolRuntime::Web {
                workspace_changed,
                ctx: web_tool_ctx,
            }
        };
        let (result, reflection_inject) =
            if handler_lookup.id_for(name.as_str()) == HandlerId::Workflow {
                workflow_tool_dispatch::dispatch_workflow_execute_tool(
                    runtime,
                    per_coord,
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    args.as_str(),
                    request_chrome_trace.clone(),
                )
                .await
            } else {
                tool_registry::dispatch_tool(tool_registry::DispatchToolParams {
                    runtime,
                    cfg,
                    effective_working_dir,
                    workspace_is_set,
                    name: &name,
                    args: &args,
                    sse_out_tx: out,
                    sse_control_mirror: sse_mirror_for_emit.as_ref(),
                    tc,
                    read_file_turn_cache: read_file_turn_cache.clone(),
                    workspace_changelist: workspace_changelist.cloned(),
                    mcp_session,
                    turn_allow,
                    long_term_memory: long_term_memory.clone(),
                    long_term_memory_scope_id: long_term_memory_scope_id.clone(),
                    handler_lookup: &handler_lookup,
                    sync_default_sandbox_backend: &sync_default_sandbox_backend,
                })
                .await
            };

        info!(
            target: super::LOG_TARGET,
            "工具调用完成 tool={} args_preview={} elapsed_ms={}",
            name,
            crate::redact::tool_arguments_preview_for_log(&args),
            t_tool.elapsed().as_millis()
        );

        serial_log_web_audit_write_tool_if_needed(
            cfg.as_ref(),
            is_readonly,
            request_audit.as_ref(),
            tracing_chat_turn.as_ref(),
            long_term_memory_scope_id.as_deref(),
            name.as_str(),
            args.as_str(),
        );

        serial_bookkeep_run_command_failure(
            per_coord,
            name.as_str(),
            args.as_str(),
            result.as_str(),
        );

        serial_maybe_invalidate_codebase_semantic_index(
            cfg,
            effective_working_dir,
            workspace_changed,
            is_readonly,
            name.as_str(),
            args.as_str(),
            result.as_str(),
        );

        serial_bookkeep_readonly_tool_ttl_cache_after_tool(SerialTtlAfterDispatchParams {
            cfg: cfg.as_ref(),
            effective_working_dir,
            readonly_tool_ttl_cache: &readonly_tool_ttl_cache,
            name: name.as_str(),
            args: args.as_str(),
            result: result.as_str(),
            workspace_changed: *workspace_changed,
        });

        if (!is_readonly || *workspace_changed)
            && let Some(c) = read_file_turn_cache.as_ref()
        {
            c.clear();
        }

        if is_readonly {
            readonly_cache.insert(cache_key, result.clone());
        } else {
            readonly_cache.clear();
        }

        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder: &tool_outcome_recorder,
            out,
            sse_control_mirror: sse_mirror_for_emit.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name: name.as_str(),
            args: args.as_str(),
            id: id.as_str(),
            result,
            reflection_inject,
        })
        .await;
    }

    ExecuteToolsBatchOutcome::Finished
}
