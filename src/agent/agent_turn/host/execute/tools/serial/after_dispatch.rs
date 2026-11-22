use std::path::Path;
use std::sync::Arc;

use log::info;

use crate::agent::per_coord::PerCoordinator;
use crate::cm_tools::tools::parse_command_spec;
use crate::tool_registry;
use crate::tool_result::parse_tool_output;

use super::super::run_command_guard::classify_run_command_failure_family_from_result;
use super::super::{emit_timeline_log_sse, emit_tool_call_summary_sse};

use super::SerialToolIterationSsePreface;
use crate::cm_names as tool_names;

pub(super) fn serial_bookkeep_run_command_failure(
    per_coord: &mut PerCoordinator,
    effective_working_dir: &Path,
    name: &str,
    args: &str,
    result: &str,
) {
    if name != tool_names::RUN_COMMAND {
        return;
    }
    let parsed = parse_tool_output(name, result);
    if parsed.ok {
        per_coord.clear_tool_failure_signature(name, args);
        per_coord.clear_tool_failure_families_for_tool(name);
        if let Some(suppress_key) =
            crate::agent::agent_turn::run_command_dedupe::run_command_duplicate_suppress_key_with_cwd(
                args,
                effective_working_dir,
            )
        {
            per_coord.record_successful_run_command(suppress_key, result.to_string());
        }
        if run_command_invocation_is_make_clean(args, effective_working_dir) {
            per_coord.clear_all_run_command_failure_state();
        }
    } else {
        let marker = parsed.error_code.clone().unwrap_or_else(|| {
            parsed
                .exit_code
                .map(|c| format!("exit_code:{c}"))
                .unwrap_or_else(|| "unknown".to_string())
        });
        per_coord.mark_tool_failure_signature(name, args, marker.clone());
        if let Some(family) =
            classify_run_command_failure_family_from_result(args, effective_working_dir, &parsed)
        {
            per_coord.mark_tool_failure_family(name, family, marker);
        }
    }
}

fn run_command_invocation_is_make_clean(args_json: &str, effective_working_dir: &Path) -> bool {
    let Ok(spec) = parse_command_spec(args_json, effective_working_dir) else {
        return false;
    };
    if spec.command != "make" {
        return false;
    }
    spec.args.iter().any(|a| a == "clean")
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
    let parsed = parse_tool_output(name, result);
    if !parsed.ok && !tool_message_has_success_evidence(result) {
        return;
    }
    if tool_registry::is_readonly_tool(cfg, name) && !workspace_changed {
        return;
    }
    per_coord.clear_all_run_command_failure_state();
}

/// 旧式（非信封）工具输出里的成功证据：本仓自产的中文 / 工具名标记，或英文 `success` 词。
///
/// 英文侧要求**词首边界**：`unsuccessful` 含 `success` 子串但不代表成功（#1478）。
/// 只卡前边界是刻意的——`successful` / `successfully` / `successes` 仍应命中。
///
/// **已知残余**：否定式表述（`not successful`）与非 ASCII 前缀（`一切success`）仍会命中。
/// 卡两侧会把 `successfully` 误伤，故保留现状；本函数只作「清除失败短路」的宽容证据，
/// 宁可放宽也不扩大误抑制。
fn tool_message_has_success_evidence(raw: &str) -> bool {
    let lower = raw.to_lowercase();
    lower.contains("已替换")
        || lower.contains("已写入")
        || lower.contains("已创建文件")
        || lower.contains(tool_names::APPLY_PATCH)
        || has_success_word(&lower)
}

fn has_success_word(lower: &str) -> bool {
    lower.match_indices("success").any(|(start, _)| {
        !lower[..start]
            .chars()
            .next_back()
            .is_some_and(|c| c.is_ascii_alphabetic())
    })
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
    request_audit: Option<&std::sync::Arc<crate::WebRequestAudit>>,
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
        control,
        cfg,
        tracing_chat_turn,
        id,
        name,
        args,
        messages,
        permission,
    } = p;
    if let Some(t) = tracing_chat_turn {
        t.record_tool_call_id_for_log(id);
    }
    emit_tool_call_summary_sse(
        control.out,
        control.sse_control_mirror.as_ref(),
        cfg.as_ref(),
        id,
        name,
        args,
        messages,
        control.sse_encoder.as_ref(),
        permission,
    )
    .await;
    emit_timeline_log_sse(
        control.out,
        control.sse_control_mirror.as_ref(),
        "tool_step_started",
        name.to_string(),
        Some(format!(
            "args={}",
            crate::redact::tool_arguments_preview_for_sse(args)
        )),
        "execute_tools::timeline tool_step_started",
        control.sse_encoder.as_ref(),
    )
    .await;
    info!(
        target: super::super::LOG_TARGET,
        "调用工具 tool={} args_preview={}",
        name,
        crate::redact::tool_arguments_preview_for_log(args)
    );
}

#[cfg(test)]
mod tests {
    use super::tool_message_has_success_evidence;

    /// #1478：`unsuccessful` 含 `success` 子串，旧实现会把它当作写成功证据。
    #[test]
    fn unsuccessful_is_not_success_evidence() {
        assert!(!tool_message_has_success_evidence(
            "patch failed: unsuccessful attempt"
        ));
    }

    /// 只卡前边界：常见成功词仍应命中。
    #[test]
    fn success_word_forms_are_evidence() {
        assert!(tool_message_has_success_evidence(
            "write completed successfully"
        ));
        assert!(tool_message_has_success_evidence("Success."));
    }

    /// 中文标记不受英文词边界改动影响。
    #[test]
    fn chinese_markers_still_count() {
        assert!(tool_message_has_success_evidence("已写入 3 处"));
        assert!(tool_message_has_success_evidence("已创建文件 a.txt"));
    }

    /// #1478 已知残余（pin 现状，不是期望行为）：否定式表述仍命中。
    ///
    /// 卡两侧会把 `successfully` 误伤，故只卡前边界。本函数只用于「清除 run_command 失败
    /// 短路」，放宽的代价是少抑制一次重试，不会掩盖真实失败。
    #[test]
    fn negated_success_is_a_known_false_positive() {
        assert!(tool_message_has_success_evidence("step was not successful"));
    }

    /// 非 ASCII 前缀不构成英文词边界（已知边界，pin 现状）。
    #[test]
    fn non_ascii_prefix_does_not_block_the_word() {
        assert!(tool_message_has_success_evidence("一切success"));
    }
}
