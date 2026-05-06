use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use log::{info, warn};
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepExecutorKind;

use crate::agent::agent_turn::sub_agent_policy::{
    executor_kind_tool_denied_body, tool_allowed_for_step_executor_kind,
};
use crate::agent::workflow_tool_dispatch;
use crate::tool_registry::{self, HandlerId, ToolRuntime};
use crate::tool_result::{ToolEnvelopeContext, parse_legacy_output};

use super::run_command_guard::{
    classify_run_command_failure_family_from_invocation,
    classify_run_command_failure_family_from_result, parse_run_command_payload,
    run_command_cargo_workdir_preflight_error, run_command_ctest_preflight_error,
};
use super::{
    ExecuteToolsBatchOutcome, ExecuteToolsCommonCtx, abort_tool_batch_if_sse_closed,
    emit_timeline_log_sse, emit_tool_call_summary_sse, emit_tool_result_sse_and_append,
};

/// 串行工具路径：统一构造 `ToolEnvelopeContext` 并下发 SSE / 追加消息。
struct SerialEmitToolResultParams<'a> {
    messages: &'a mut Vec<crate::types::Message>,
    per_coord: &'a mut PerCoordinator,
    cfg: &'a Arc<crate::config::AgentConfig>,
    tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    name: &'a str,
    args: &'a str,
    id: &'a str,
    result: String,
    reflection_inject: Option<serde_json::Value>,
}

struct SerialTtlRunCommandEarlyHitParams<'a> {
    messages: &'a mut Vec<crate::types::Message>,
    per_coord: &'a mut PerCoordinator,
    cfg: &'a Arc<crate::config::AgentConfig>,
    tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    effective_working_dir: &'a Path,
    name: &'a str,
    args: &'a str,
    id: &'a str,
    readonly_tool_ttl_cache: &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
}

async fn serial_try_ttl_run_command_cache_hit(p: SerialTtlRunCommandEarlyHitParams<'_>) -> bool {
    let ttl_secs = p.cfg.chat_queues_cache.readonly_tool_ttl_cache_secs;
    if ttl_secs == 0
        || p.name != "run_command"
        || !crate::readonly_tool_ttl_cache::run_command_invocation_ttl_cache_eligible(p.args)
    {
        return false;
    }
    let ws_key = p.effective_working_dir.to_string_lossy();
    let Some(cached) = p
        .readonly_tool_ttl_cache
        .try_get(ws_key.as_ref(), p.name, p.args)
    else {
        return false;
    };
    let body = format!("[只读命令短时缓存命中 · TTL≤{ttl_secs}s]\n{cached}");
    info!(
        target: super::LOG_TARGET,
        "run_command TTL 缓存命中 args_preview={}",
        crate::redact::tool_arguments_preview_for_log(p.args)
    );
    emit_serial_tool_result(SerialEmitToolResultParams {
        messages: p.messages,
        per_coord: p.per_coord,
        cfg: p.cfg,
        tool_outcome_recorder: p.tool_outcome_recorder,
        out: p.out,
        echo_terminal_transcript: p.echo_terminal_transcript,
        terminal_tool_display_max_chars: p.terminal_tool_display_max_chars,
        tool_result_envelope_v1: p.tool_result_envelope_v1,
        name: p.name,
        args: p.args,
        id: p.id,
        result: body,
        reflection_inject: None,
    })
    .await;
    true
}

fn readonly_tool_ttl_cache_should_invalidate_workspace(
    cfg: &crate::config::AgentConfig,
    name: &str,
    args: &str,
    workspace_changed: bool,
) -> bool {
    workspace_changed
        || if name == "run_command" {
            !crate::readonly_tool_ttl_cache::run_command_invocation_ttl_cache_eligible(args)
        } else {
            !tool_registry::is_readonly_tool(cfg, name)
        }
}

struct SerialEarlyToolPolicyDenyParams<'a> {
    messages: &'a mut Vec<crate::types::Message>,
    per_coord: &'a mut PerCoordinator,
    cfg: &'a Arc<crate::config::AgentConfig>,
    tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    name: &'a str,
    args: &'a str,
    id: &'a str,
    step_executor_constraint: Option<PlanStepExecutorKind>,
    tools_defs_full: &'a [crate::types::Tool],
    turn_allow: Option<&'a HashSet<String>>,
}

async fn serial_emit_early_tool_policy_denials(p: SerialEarlyToolPolicyDenyParams<'_>) -> bool {
    if let Some(k) = p.step_executor_constraint
        && !tool_allowed_for_step_executor_kind(p.cfg.as_ref(), p.name, k)
    {
        let denied = executor_kind_tool_denied_body(p.cfg.as_ref(), p.tools_defs_full, p.name, k);
        warn!(target: super::LOG_TARGET, "{}", denied);
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages: p.messages,
            per_coord: p.per_coord,
            cfg: p.cfg,
            tool_outcome_recorder: p.tool_outcome_recorder,
            out: p.out,
            echo_terminal_transcript: p.echo_terminal_transcript,
            terminal_tool_display_max_chars: p.terminal_tool_display_max_chars,
            tool_result_envelope_v1: p.tool_result_envelope_v1,
            name: p.name,
            args: p.args,
            id: p.id,
            result: denied,
            reflection_inject: None,
        })
        .await;
        return true;
    }

    if !crate::agent_role_turn::tool_allowed_for_turn(p.name, p.turn_allow) {
        let denied = crate::agent_role_turn::turn_tool_denied_message(p.name);
        warn!(target: super::LOG_TARGET, "{}", denied);
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages: p.messages,
            per_coord: p.per_coord,
            cfg: p.cfg,
            tool_outcome_recorder: p.tool_outcome_recorder,
            out: p.out,
            echo_terminal_transcript: p.echo_terminal_transcript,
            terminal_tool_display_max_chars: p.terminal_tool_display_max_chars,
            tool_result_envelope_v1: p.tool_result_envelope_v1,
            name: p.name,
            args: p.args,
            id: p.id,
            result: denied,
            reflection_inject: None,
        })
        .await;
        return true;
    }

    false
}

struct SerialTtlAfterDispatchParams<'a> {
    cfg: &'a crate::config::AgentConfig,
    effective_working_dir: &'a Path,
    readonly_tool_ttl_cache: &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
    name: &'a str,
    args: &'a str,
    result: &'a str,
    workspace_changed: bool,
}

fn serial_bookkeep_readonly_tool_ttl_cache_after_tool(p: SerialTtlAfterDispatchParams<'_>) {
    let ws_key = p.effective_working_dir.to_string_lossy();
    let ttl_secs = p.cfg.chat_queues_cache.readonly_tool_ttl_cache_secs;
    let mut ttl_run_command_success_cache: Option<String> = None;
    if ttl_secs > 0 && p.name == "run_command" {
        let parsed_tool = parse_legacy_output(p.name, p.result);
        if parsed_tool.ok
            && crate::readonly_tool_ttl_cache::run_command_invocation_ttl_cache_eligible(p.args)
        {
            ttl_run_command_success_cache = Some(p.result.to_string());
        } else {
            p.readonly_tool_ttl_cache
                .remove(ws_key.as_ref(), p.name, p.args);
        }
    }

    if readonly_tool_ttl_cache_should_invalidate_workspace(
        p.cfg,
        p.name,
        p.args,
        p.workspace_changed,
    ) {
        p.readonly_tool_ttl_cache
            .invalidate_workspace(ws_key.as_ref());
    }

    if let Some(out) = ttl_run_command_success_cache {
        p.readonly_tool_ttl_cache.insert(
            ws_key.as_ref(),
            p.name,
            p.args,
            out,
            Duration::from_secs(ttl_secs),
            p.cfg.chat_queues_cache.readonly_tool_ttl_cache_max_entries,
        );
    }
}

async fn emit_serial_tool_result(p: SerialEmitToolResultParams<'_>) {
    let SerialEmitToolResultParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        name,
        args,
        id,
        result,
        reflection_inject,
    } = p;
    let env = ToolEnvelopeContext {
        tool_call_id: id,
        execution_mode: "serial",
        parallel_batch_id: None,
    };
    emit_tool_result_sse_and_append(
        messages,
        per_coord,
        super::EmitToolResultParams {
            cfg,
            tool_outcome_recorder,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
            result,
            reflection_inject,
            envelope_ctx: Some(env),
        },
    )
    .await;
}

/// 预检 / 策略拒绝 / `run_command` 短路 / 只读缓存命中：若已下发工具结果则返回 `true`（外层应 `continue`）。
struct SerialEmitEarlyWithoutDispatchParams<'a> {
    messages: &'a mut Vec<crate::types::Message>,
    per_coord: &'a mut PerCoordinator,
    cfg: &'a Arc<crate::config::AgentConfig>,
    tool_outcome_recorder: &'a Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a mpsc::Sender<String>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    effective_working_dir: &'a Path,
    name: &'a str,
    args: &'a str,
    id: &'a str,
    step_executor_constraint: Option<PlanStepExecutorKind>,
    tools_defs_full: &'a [crate::types::Tool],
    turn_allow: Option<&'a HashSet<String>>,
    readonly_cache: &'a mut HashMap<(String, String), String>,
    readonly_tool_ttl_cache: &'a Arc<crate::readonly_tool_ttl_cache::ReadonlyToolTtlCache>,
}

async fn serial_emit_early_without_dispatch(p: SerialEmitEarlyWithoutDispatchParams<'_>) -> bool {
    let SerialEmitEarlyWithoutDispatchParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        effective_working_dir,
        name,
        args,
        id,
        step_executor_constraint,
        tools_defs_full,
        turn_allow,
        readonly_cache,
        readonly_tool_ttl_cache,
    } = p;
    if let Some(preflight_error) =
        run_command_cargo_workdir_preflight_error(name, args, effective_working_dir)
    {
        per_coord.mark_tool_failure_signature(name, args, "cargo_manifest_missing".to_string());
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
            result: preflight_error,
            reflection_inject: None,
        })
        .await;
        return true;
    }
    if let Some(preflight_error) = run_command_ctest_preflight_error(name, args) {
        per_coord.mark_tool_failure_signature(name, args, "ctest_dash_c_build_misuse".to_string());
        per_coord.mark_tool_failure_family(
            name,
            "ctest_dash_c_build_misuse",
            "ctest_dash_c_build_misuse".to_string(),
        );
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
            result: preflight_error,
            reflection_inject: None,
        })
        .await;
        return true;
    }

    if serial_emit_early_tool_policy_denials(SerialEarlyToolPolicyDenyParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        name,
        args,
        id,
        step_executor_constraint,
        tools_defs_full,
        turn_allow,
    })
    .await
    {
        return true;
    }

    let is_readonly = tool_registry::is_readonly_tool(cfg.as_ref(), name);
    let cache_key = (name.to_string(), args.to_string());

    if name == "run_command"
        && let Some(prev_error) = per_coord.repeated_tool_failure_error_marker(name, args)
    {
        let short_circuit = format!(
            "错误：检测到同命令重复失败，已短路本次调用（error={prev_error}）。请切换策略（例如调整工作目录、改用 --manifest-path、或先做目录/文件探测）。"
        );
        warn!(
            target: super::LOG_TARGET,
            "run_command 重复失败短路 args_preview={} prev_error={}",
            crate::redact::tool_arguments_preview_for_log(args),
            prev_error
        );
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
            result: short_circuit,
            reflection_inject: None,
        })
        .await;
        return true;
    }
    if name == "run_command"
        && let Some((command, command_args)) = parse_run_command_payload(args)
        && let Some(family) = classify_run_command_failure_family_from_invocation(
            command.as_str(),
            command_args.as_slice(),
        )
        && let Some(prev_error) = per_coord.repeated_tool_failure_family_marker(name, family)
    {
        let short_circuit = format!(
            "错误：检测到同类失败已发生（family={family}, prev_error={prev_error}），已短路本次调用。请直接切换策略，避免继续同类试探。"
        );
        warn!(
            target: super::LOG_TARGET,
            "run_command 同类失败短路 family={} args_preview={} prev_error={}",
            family,
            crate::redact::tool_arguments_preview_for_log(args),
            prev_error
        );
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
            result: short_circuit,
            reflection_inject: None,
        })
        .await;
        return true;
    }

    if serial_try_ttl_run_command_cache_hit(SerialTtlRunCommandEarlyHitParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        effective_working_dir,
        name,
        args,
        id,
        readonly_tool_ttl_cache,
    })
    .await
    {
        return true;
    }

    if is_readonly && let Some(cached) = readonly_cache.get(&cache_key) {
        info!(
            target: super::LOG_TARGET,
            "工具结果命中缓存（只读去重） tool={} args_preview={}",
            name,
            crate::redact::tool_arguments_preview_for_log(args)
        );
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
            result: cached.clone(),
            reflection_inject: None,
        })
        .await;
        return true;
    }

    false
}

fn serial_bookkeep_run_command_failure(
    per_coord: &mut PerCoordinator,
    name: &str,
    args: &str,
    result: &str,
) {
    if name != "run_command" {
        return;
    }
    let parsed = parse_legacy_output(name, result);
    if parsed.ok {
        per_coord.clear_tool_failure_signature(name, args);
        per_coord.clear_tool_failure_families_for_tool(name);
    } else {
        let marker = parsed.error_code.unwrap_or_else(|| {
            parsed
                .exit_code
                .map(|c| format!("exit_code:{c}"))
                .unwrap_or_else(|| "unknown".to_string())
        });
        per_coord.mark_tool_failure_signature(name, args, marker.clone());
        if let Some(family) = classify_run_command_failure_family_from_result(result) {
            per_coord.mark_tool_failure_family(name, family, marker);
        }
    }
}

fn serial_maybe_invalidate_codebase_semantic_index(
    cfg: &Arc<crate::config::AgentConfig>,
    effective_working_dir: &Path,
    workspace_changed: &mut bool,
    is_readonly: bool,
    name: &str,
    args: &str,
    result: &str,
) {
    if !cfg.codebase_semantic.codebase_semantic_search_enabled
        || !cfg
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change
    {
        return;
    }
    let cs = crate::memory::codebase_semantic_index::CodebaseSemanticToolParams::from_agent_config(
        cfg.as_ref(),
    );
    if !cs.enabled || !cs.invalidate_on_workspace_change {
        return;
    }
    let should_apply = if is_readonly {
        *workspace_changed
    } else {
        crate::memory::codebase_semantic_invalidation::tool_output_semantic_success(name, result)
    };
    if !should_apply {
        return;
    }
    let inv = crate::memory::codebase_semantic_invalidation::invalidation_for_tool_call(
        cfg.as_ref(),
        name,
        args,
    )
    .or_else(|| {
        (*workspace_changed).then_some(
            crate::memory::codebase_semantic_invalidation::CodebaseSemanticInvalidation::FullWorkspace,
        )
    });
    if let Some(inv) = inv {
        crate::memory::codebase_semantic_invalidation::apply_after_successful_tool(
            effective_working_dir,
            cs.index_sqlite_path.as_str(),
            inv,
        );
    }
}

fn serial_log_web_audit_write_tool_if_needed(
    cfg: &crate::config::AgentConfig,
    is_readonly: bool,
    request_audit: Option<&std::sync::Arc<crate::web::audit::WebRequestAudit>>,
    tracing_chat_turn: Option<&std::sync::Arc<crate::observability::TracingChatTurn>>,
    long_term_memory_scope_id: Option<&str>,
    name: &str,
    args: &str,
) {
    if !is_readonly
        && cfg.web_api.web_audit_log_write_tools
        && let Some(audit) = request_audit
    {
        let ts_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let job_id = tracing_chat_turn.map(|t| t.job_id).unwrap_or(0);
        let scope = long_term_memory_scope_id.unwrap_or("");
        info!(
            target: "crabmate::audit_write_tool",
            "audit_write_tool ts_ms={} job_id={} conversation_id={} source={} client_ip={} peer_ip={} bearer_fp={} tool={} args_preview={}",
            ts_ms,
            job_id,
            scope,
            audit.source,
            audit.client_ip,
            audit.peer_ip,
            audit.bearer_fp.as_deref().unwrap_or("-"),
            name,
            crate::redact::tool_arguments_preview_for_log(args),
        );
    }
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
    } = ctx;

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
        if let Some(ref t) = tracing_chat_turn {
            t.record_tool_call_id_for_log(id.as_str());
        }
        emit_tool_call_summary_sse(out, cfg.as_ref(), id.as_str(), &name, &args, messages).await;
        emit_timeline_log_sse(
            out,
            "tool_step_started",
            name.clone(),
            Some(format!(
                "args={}",
                crate::redact::tool_arguments_preview_for_sse(&args)
            )),
            "execute_tools::timeline tool_step_started",
        )
        .await;
        info!(
            target: super::LOG_TARGET,
            "调用工具 tool={} args_preview={}",
            name,
            crate::redact::tool_arguments_preview_for_log(&args)
        );

        if serial_emit_early_without_dispatch(SerialEmitEarlyWithoutDispatchParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder: &tool_outcome_recorder,
            out,
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
