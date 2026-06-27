use std::time::Duration;

use log::{info, warn};

use crabmate_agent::agent_turn::{ToolPolicyEarlyDenyParams, tool_policy_early_deny_message};

use crate::agent::per_coord::PerCoordinator;
use crate::tool_registry;
use crate::tool_result::parse_legacy_output;

use super::super::emit_tool_result_sse_and_append;
use super::super::run_command_guard::{
    classify_run_command_failure_family_from_invocation, parse_run_command_payload,
    run_command_cargo_workdir_preflight_error, run_command_ctest_preflight_error,
};

use super::{
    SerialEarlyToolPolicyDenyParams, SerialEmitEarlyWithoutDispatchParams,
    SerialEmitToolResultParams, SerialTtlAfterDispatchParams, SerialTtlRunCommandEarlyHitParams,
};

pub(super) async fn serial_try_ttl_run_command_cache_hit(
    p: SerialTtlRunCommandEarlyHitParams<'_>,
) -> bool {
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
        target: super::super::LOG_TARGET,
        "run_command TTL 缓存命中 args_preview={}",
        crate::redact::tool_arguments_preview_for_log(p.args)
    );
    emit_serial_tool_result(SerialEmitToolResultParams {
        messages: p.messages,
        per_coord: p.per_coord,
        cfg: p.cfg,
        tool_outcome_recorder: p.tool_outcome_recorder,
        out: p.out,
        sse_control_mirror: p.sse_control_mirror.clone(),
        clarification_questionnaire_hook: p.clarification_questionnaire_hook.clone(),
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

pub(super) async fn serial_emit_early_tool_policy_denials(
    p: SerialEarlyToolPolicyDenyParams<'_>,
) -> bool {
    if let Some(denied) = tool_policy_early_deny_message(&ToolPolicyEarlyDenyParams {
        cfg: p.cfg.as_ref(),
        name: p.name,
        step_executor_constraint: p.step_executor_constraint,
        tools_defs: p.tools_defs_full,
        turn_allow: p.turn_allow,
    }) {
        warn!(target: super::super::LOG_TARGET, "{}", denied);
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages: p.messages,
            per_coord: p.per_coord,
            cfg: p.cfg,
            tool_outcome_recorder: p.tool_outcome_recorder,
            out: p.out,
            sse_control_mirror: p.sse_control_mirror.clone(),
            clarification_questionnaire_hook: p.clarification_questionnaire_hook.clone(),
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

pub(super) fn serial_bookkeep_readonly_tool_ttl_cache_after_tool(
    p: SerialTtlAfterDispatchParams<'_>,
) {
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

pub(super) async fn emit_serial_tool_result(p: SerialEmitToolResultParams<'_>) {
    let SerialEmitToolResultParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        sse_control_mirror,
        clarification_questionnaire_hook,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        name,
        args,
        id,
        result,
        reflection_inject,
    } = p;
    let env = crate::tool_result::ToolEnvelopeContext {
        tool_call_id: id,
        execution_mode: "serial",
        parallel_batch_id: None,
    };
    emit_tool_result_sse_and_append(
        messages,
        per_coord,
        super::super::EmitToolResultParams {
            cfg,
            tool_outcome_recorder,
            out,
            sse_control_mirror,
            clarification_questionnaire_hook,
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

fn mark_ctest_preflight_failure_signature(per_coord: &mut PerCoordinator, name: &str, args: &str) {
    per_coord.mark_tool_failure_signature(name, args, "ctest_dash_c_build_misuse".to_string());
    per_coord.mark_tool_failure_family(
        name,
        "ctest_dash_c_build_misuse",
        "ctest_dash_c_build_misuse".to_string(),
    );
}

struct SerialRunCommandDupShortCircuitEmitCtx<'a> {
    messages: &'a mut Vec<crate::types::Message>,
    per_coord: &'a mut PerCoordinator,
    cfg: &'a std::sync::Arc<crate::config::AgentConfig>,
    tool_outcome_recorder: &'a std::sync::Arc<crate::tool_stats::ToolOutcomeRecorder>,
    out: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<crate::sse::SseControlMirror>,
    clarification_questionnaire_hook:
        Option<std::sync::Arc<dyn Fn(crate::sse::ClarificationQuestionnaireBody) + Send + Sync>>,
    echo_terminal_transcript: bool,
    terminal_tool_display_max_chars: usize,
    tool_result_envelope_v1: bool,
    name: &'a str,
    args: &'a str,
    id: &'a str,
}

/// `run_command` 重复失败短路（签名一致 / 同类 family），从 [`serial_emit_early_without_dispatch`] 拆出以降低 lizard nloc。
async fn serial_emit_run_command_failure_short_circuits(
    p: SerialRunCommandDupShortCircuitEmitCtx<'_>,
) -> bool {
    let SerialRunCommandDupShortCircuitEmitCtx {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        sse_control_mirror,
        clarification_questionnaire_hook,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        name,
        args,
        id,
    } = p;
    if let Some(prev_error) = per_coord.repeated_tool_failure_error_marker(name, args) {
        let short_circuit = format!(
            "错误：检测到同命令重复失败，已短路本次调用（error={prev_error}）。请切换策略（例如调整工作目录、改用 --manifest-path、或先做目录/文件探测）。"
        );
        warn!(
            target: super::super::LOG_TARGET,
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
            sse_control_mirror: sse_control_mirror.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
    if let Some((command, command_args)) = parse_run_command_payload(args)
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
            target: super::super::LOG_TARGET,
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
            sse_control_mirror: sse_control_mirror.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
    false
}

/// 预检 / 策略拒绝 / `run_command` 短路 / 只读缓存命中：若已下发工具结果则返回 `true`（外层应 `continue`）。
pub(super) async fn serial_emit_early_without_dispatch(
    p: SerialEmitEarlyWithoutDispatchParams<'_>,
) -> bool {
    let SerialEmitEarlyWithoutDispatchParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        sse_control_mirror,
        clarification_questionnaire_hook,
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
            sse_control_mirror: sse_control_mirror.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
        mark_ctest_preflight_failure_signature(per_coord, name, args);
        emit_serial_tool_result(SerialEmitToolResultParams {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            sse_control_mirror: sse_control_mirror.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
        sse_control_mirror: sse_control_mirror.clone(),
        clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
        && serial_emit_run_command_failure_short_circuits(SerialRunCommandDupShortCircuitEmitCtx {
            messages,
            per_coord,
            cfg,
            tool_outcome_recorder,
            out,
            sse_control_mirror: sse_control_mirror.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
            echo_terminal_transcript,
            terminal_tool_display_max_chars,
            tool_result_envelope_v1,
            name,
            args,
            id,
        })
        .await
    {
        return true;
    }

    if serial_try_ttl_run_command_cache_hit(SerialTtlRunCommandEarlyHitParams {
        messages,
        per_coord,
        cfg,
        tool_outcome_recorder,
        out,
        sse_control_mirror: sse_control_mirror.clone(),
        clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
            target: super::super::LOG_TARGET,
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
            sse_control_mirror: sse_control_mirror.clone(),
            clarification_questionnaire_hook: clarification_questionnaire_hook.clone(),
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
