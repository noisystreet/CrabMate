//! 在 [`super::finalize::finalize`] 前对 `ConfigBuilder` 做显式数值范围校验。
//! 与 `finalize` 中的 `clamp` 区分：**越界则报错**，避免静默截断导致与运维预期不符。

use super::builder::ConfigBuilder;

fn err_out_of_range(
    key: &str,
    v: impl std::fmt::Display,
    min: impl std::fmt::Display,
    max: impl std::fmt::Display,
) -> String {
    format!(
        "配置错误：{key}={v} 超出允许范围 [{min}, {max}]（请修正 TOML 或对应 AGENT_* 环境变量；不再静默截断）"
    )
}

fn check_u64_inclusive(key: &str, v: Option<u64>, min: u64, max: u64) -> Result<(), String> {
    let Some(n) = v else {
        return Ok(());
    };
    if n < min || n > max {
        return Err(err_out_of_range(key, n, min, max));
    }
    Ok(())
}

fn check_i64_inclusive(key: &str, v: Option<i64>, min: i64, max: i64) -> Result<(), String> {
    let Some(n) = v else {
        return Ok(());
    };
    if n < min || n > max {
        return Err(err_out_of_range(key, n, min, max));
    }
    Ok(())
}

fn check_f64_inclusive(key: &str, v: Option<f64>, min: f64, max: f64) -> Result<(), String> {
    let Some(n) = v else {
        return Ok(());
    };
    if !n.is_finite() {
        return Err(format!(
            "配置错误：{key}={n} 不是有限浮点数（请修正 TOML 或 AGENT_TEMPERATURE）"
        ));
    }
    if n < min || n > max {
        return Err(err_out_of_range(key, n, min, max));
    }
    Ok(())
}

/// 对 `finalize` 中会 `clamp` 的字段做前置校验；`None` 表示使用默认，跳过。
pub(super) fn validate_builder_numeric_ranges(b: &ConfigBuilder) -> Result<(), String> {
    check_u64_inclusive("max_message_history", b.max_message_history, 1, 1024)?;
    check_u64_inclusive(
        "tui_session_max_messages",
        b.tui_session_max_messages,
        2,
        50_000,
    )?;
    check_u64_inclusive("command_timeout_secs", b.command_timeout_secs, 1, u64::MAX)?;
    check_u64_inclusive(
        "command_max_output_len",
        b.command_max_output_len,
        1024,
        131_072,
    )?;
    check_u64_inclusive("max_tokens", b.max_tokens, 256, 32_768)?;
    check_f64_inclusive("temperature", b.temperature, 0.0, 2.0)?;
    check_i64_inclusive("llm_seed", b.llm_seed, i64::MIN, i64::MAX)?;

    check_u64_inclusive("api_timeout_secs", b.api_timeout_secs, 1, u64::MAX)?;
    check_u64_inclusive("api_max_retries", b.api_max_retries, 0, 10)?;
    check_u64_inclusive("api_retry_delay_secs", b.api_retry_delay_secs, 1, u64::MAX)?;
    check_u64_inclusive("weather_timeout_secs", b.weather_timeout_secs, 1, u64::MAX)?;
    check_u64_inclusive(
        "web_search_timeout_secs",
        b.web_search_timeout_secs,
        1,
        u64::MAX,
    )?;
    check_u64_inclusive("web_search_max_results", b.web_search_max_results, 1, 20)?;

    check_u64_inclusive(
        "http_fetch_timeout_secs",
        b.http_fetch_timeout_secs,
        1,
        u64::MAX,
    )?;
    check_u64_inclusive(
        "http_fetch_max_response_bytes",
        b.http_fetch_max_response_bytes,
        1024,
        4_194_304,
    )?;

    check_u64_inclusive(
        "reflection_default_max_rounds",
        b.reflection_default_max_rounds,
        1,
        u64::MAX,
    )?;
    check_u64_inclusive(
        "plan_rewrite_max_attempts",
        b.plan_rewrite_max_attempts,
        1,
        20,
    )?;
    check_u64_inclusive(
        "final_plan_semantic_check_max_non_readonly_tools",
        b.final_plan_semantic_check_max_non_readonly_tools,
        0,
        32,
    )?;
    check_u64_inclusive(
        "final_plan_semantic_check_max_tokens",
        b.final_plan_semantic_check_max_tokens,
        32,
        1024,
    )?;

    check_u64_inclusive(
        "cursor_rules_max_chars",
        b.cursor_rules_max_chars,
        1024,
        1_000_000,
    )?;
    check_u64_inclusive(
        "tool_message_max_chars",
        b.tool_message_max_chars,
        1024,
        1_048_576,
    )?;

    check_u64_inclusive(
        "agent_tool_stats_window_events",
        b.agent_tool_stats_window_events,
        16,
        65_536,
    )?;
    check_u64_inclusive(
        "agent_tool_stats_min_samples",
        b.agent_tool_stats_min_samples,
        1,
        10_000,
    )?;
    check_u64_inclusive(
        "agent_tool_stats_max_chars",
        b.agent_tool_stats_max_chars,
        64,
        32_768,
    )?;
    check_f64_inclusive(
        "agent_tool_stats_warn_below_success_ratio",
        b.agent_tool_stats_warn_below_success_ratio,
        0.0,
        1.0,
    )?;

    check_u64_inclusive("context_char_budget", b.context_char_budget, 0, 50_000_000)?;
    check_u64_inclusive(
        "context_min_messages_after_system",
        b.context_min_messages_after_system,
        1,
        128,
    )?;
    check_u64_inclusive(
        "context_summary_trigger_chars",
        b.context_summary_trigger_chars,
        0,
        50_000_000,
    )?;
    check_u64_inclusive(
        "context_summary_tail_messages",
        b.context_summary_tail_messages,
        4,
        64,
    )?;
    check_u64_inclusive(
        "context_summary_max_tokens",
        b.context_summary_max_tokens,
        256,
        8192,
    )?;
    check_u64_inclusive(
        "context_summary_transcript_max_chars",
        b.context_summary_transcript_max_chars,
        10_000,
        2_000_000,
    )?;

    check_u64_inclusive(
        "health_llm_models_probe_cache_secs",
        b.health_llm_models_probe_cache_secs,
        5,
        86_400,
    )?;

    check_u64_inclusive(
        "chat_queue_max_concurrent",
        b.chat_queue_max_concurrent,
        1,
        256,
    )?;
    check_u64_inclusive("chat_queue_max_pending", b.chat_queue_max_pending, 1, 8192)?;
    check_u64_inclusive(
        "parallel_readonly_tools_max",
        b.parallel_readonly_tools_max,
        1,
        256,
    )?;
    check_u64_inclusive(
        "read_file_turn_cache_max_entries",
        b.read_file_turn_cache_max_entries,
        0,
        4096,
    )?;

    check_u64_inclusive(
        "test_result_cache_max_entries",
        b.test_result_cache_max_entries,
        1,
        512,
    )?;

    check_u64_inclusive(
        "session_workspace_changelist_max_chars",
        b.session_workspace_changelist_max_chars,
        0,
        500_000,
    )?;

    check_u64_inclusive(
        "staged_plan_patch_max_attempts",
        b.staged_plan_patch_max_attempts,
        1,
        16,
    )?;
    check_u64_inclusive(
        "staged_plan_ensemble_count",
        b.staged_plan_ensemble_count,
        1,
        3,
    )?;

    check_u64_inclusive(
        "sync_default_tool_sandbox_docker_timeout_secs",
        b.sync_default_tool_sandbox_docker_timeout_secs,
        1,
        u64::MAX,
    )?;

    check_u64_inclusive(
        "agent_memory_file_max_chars",
        b.agent_memory_file_max_chars,
        256,
        500_000,
    )?;
    check_u64_inclusive(
        "living_docs_inject_max_chars",
        b.living_docs_inject_max_chars,
        0,
        500_000,
    )?;
    check_u64_inclusive(
        "living_docs_file_max_each_chars",
        b.living_docs_file_max_each_chars,
        0,
        500_000,
    )?;
    check_u64_inclusive(
        "project_profile_inject_max_chars",
        b.project_profile_inject_max_chars,
        0,
        500_000,
    )?;
    check_u64_inclusive(
        "project_dependency_brief_inject_max_chars",
        b.project_dependency_brief_inject_max_chars,
        0,
        500_000,
    )?;

    check_u64_inclusive(
        "tool_call_explain_min_chars",
        b.tool_call_explain_min_chars,
        1,
        256,
    )?;
    check_u64_inclusive(
        "tool_call_explain_max_chars",
        b.tool_call_explain_max_chars,
        1,
        4000,
    )?;

    check_u64_inclusive(
        "long_term_memory_max_entries",
        b.long_term_memory_max_entries,
        1,
        100_000,
    )?;
    check_u64_inclusive(
        "long_term_memory_inject_max_chars",
        b.long_term_memory_inject_max_chars,
        256,
        500_000,
    )?;
    check_u64_inclusive("long_term_memory_top_k", b.long_term_memory_top_k, 1, 64)?;
    check_u64_inclusive(
        "long_term_memory_max_chars_per_chunk",
        b.long_term_memory_max_chars_per_chunk,
        256,
        32_000,
    )?;
    check_u64_inclusive(
        "long_term_memory_min_chars_to_index",
        b.long_term_memory_min_chars_to_index,
        0,
        4096,
    )?;
    check_u64_inclusive(
        "long_term_memory_default_ttl_secs",
        b.long_term_memory_default_ttl_secs,
        0,
        365 * 86400 * 10,
    )?;

    check_u64_inclusive(
        "mcp_tool_timeout_secs",
        b.mcp_tool_timeout_secs,
        1,
        u64::MAX,
    )?;

    check_u64_inclusive(
        "codebase_semantic_max_file_bytes",
        b.codebase_semantic_max_file_bytes,
        4096,
        4 * 1024 * 1024,
    )?;
    check_u64_inclusive(
        "codebase_semantic_chunk_max_chars",
        b.codebase_semantic_chunk_max_chars,
        256,
        16_000,
    )?;
    check_u64_inclusive("codebase_semantic_top_k", b.codebase_semantic_top_k, 1, 64)?;
    check_u64_inclusive(
        "codebase_semantic_query_max_chunks",
        b.codebase_semantic_query_max_chunks,
        0,
        2_000_000,
    )?;
    check_u64_inclusive(
        "codebase_semantic_rebuild_max_files",
        b.codebase_semantic_rebuild_max_files,
        1,
        100_000,
    )?;

    check_u64_inclusive(
        "tool_registry_http_fetch_wall_timeout_secs",
        b.tool_registry_http_fetch_wall_timeout_secs,
        1,
        86_400,
    )?;
    check_u64_inclusive(
        "tool_registry_http_request_wall_timeout_secs",
        b.tool_registry_http_request_wall_timeout_secs,
        1,
        86_400,
    )?;

    for (k, v) in &b.tool_registry_parallel_wall_timeout_secs {
        if *v < 1 || *v > 86_400 {
            return Err(err_out_of_range(
                &format!("tool_registry.parallel_wall_timeout_secs[{k}]"),
                *v,
                1,
                86_400,
            ));
        }
    }

    if let Some(ref min_c) = b.tool_call_explain_min_chars
        && let Some(ref max_c) = b.tool_call_explain_max_chars
        && max_c < min_c
    {
        return Err(format!(
            "配置错误：tool_call_explain_max_chars({max_c}) 小于 tool_call_explain_min_chars({min_c})"
        ));
    }

    Ok(())
}
