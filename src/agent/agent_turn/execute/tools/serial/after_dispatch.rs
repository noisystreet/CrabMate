use std::path::Path;
use std::sync::Arc;

use log::info;

use crate::agent::per_coord::PerCoordinator;
use crate::tool_registry;
use crate::tool_result::parse_legacy_output;

use super::super::run_command_guard::classify_run_command_failure_family_from_result;
use super::super::run_command_guard::parse_run_command_payload;
use super::super::{emit_timeline_log_sse, emit_tool_call_summary_sse};

use super::SerialToolIterationSsePreface;

pub(super) fn serial_bookkeep_run_command_failure(
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
        if run_command_invocation_is_make_clean(args) {
            per_coord.clear_all_run_command_failure_state();
        }
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

fn run_command_invocation_is_make_clean(args_json: &str) -> bool {
    let Some((command, command_args)) = parse_run_command_payload(args_json) else {
        return false;
    };
    if command != "make" {
        return false;
    }
    command_args.iter().any(|a| a == "clean")
}

/// 工作区写操作成功后清除 `run_command` 失败短路，便于 patch 后重试 make/cmake。
pub(super) fn serial_clear_run_command_failures_after_workspace_mutation(
    per_coord: &mut PerCoordinator,
    cfg: &crate::config::AgentConfig,
    is_readonly: bool,
    workspace_changed: bool,
    name: &str,
    result: &str,
) {
    if is_readonly && !workspace_changed {
        return;
    }
    let parsed = parse_legacy_output(name, result);
    if !parsed.ok && !tool_message_has_success_evidence(result) {
        return;
    }
    if tool_registry::is_readonly_tool(cfg, name) && !workspace_changed {
        return;
    }
    per_coord.clear_all_run_command_failure_state();
}

fn tool_message_has_success_evidence(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("已替换")
        || lower.contains("已写入")
        || lower.contains("已创建文件")
        || lower.contains("apply_patch")
        || lower.contains("success")
}

pub(super) fn serial_maybe_invalidate_codebase_semantic_index(
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

pub(super) fn serial_log_web_audit_write_tool_if_needed(
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

pub(super) async fn serial_tool_iteration_sse_preface(p: SerialToolIterationSsePreface<'_>) {
    let SerialToolIterationSsePreface {
        out,
        sse_mirror,
        cfg,
        tracing_chat_turn,
        id,
        name,
        args,
        messages,
    } = p;
    if let Some(t) = tracing_chat_turn {
        t.record_tool_call_id_for_log(id);
    }
    emit_tool_call_summary_sse(out, sse_mirror, cfg.as_ref(), id, name, args, messages).await;
    emit_timeline_log_sse(
        out,
        sse_mirror,
        "tool_step_started",
        name.to_string(),
        Some(format!(
            "args={}",
            crate::redact::tool_arguments_preview_for_sse(args)
        )),
        "execute_tools::timeline tool_step_started",
    )
    .await;
    info!(
        target: super::super::LOG_TARGET,
        "调用工具 tool={} args_preview={}",
        name,
        crate::redact::tool_arguments_preview_for_log(args)
    );
}
