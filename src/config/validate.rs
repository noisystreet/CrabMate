//! 在 [`super::finalize::finalize`] 前对 `ConfigBuilder` 做显式数值范围校验。
//! 与 `finalize` 中的 `clamp` 区分：**越界则报错**，避免静默截断导致与运维预期不符。

use super::builder::ConfigBuilder;

type U64RangeRow = (&'static str, fn(&ConfigBuilder) -> Option<u64>, u64, u64);
type F64RangeRow = (&'static str, fn(&ConfigBuilder) -> Option<f64>, f64, f64);

fn err_out_of_range(
    key: &str,
    v: impl std::fmt::Display,
    min: impl std::fmt::Display,
    max: impl std::fmt::Display,
) -> String {
    format!(
        "配置错误：{key}={v} 超出允许范围 [{min}, {max}]（请修正 TOML 或对应 CM_* 环境变量；不再静默截断）"
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
            "配置错误：{key}={n} 不是有限浮点数（请修正 TOML 或 CM_TEMPERATURE）"
        ));
    }
    if n < min || n > max {
        return Err(err_out_of_range(key, n, min, max));
    }
    Ok(())
}

fn validate_u64_table(b: &ConfigBuilder) -> Result<(), String> {
    const ROWS: &[U64RangeRow] = &[
        ("max_message_history", |b| b.max_message_history, 1, 1024),
        (
            "tui_session_max_messages",
            |b| b.tui_session_max_messages,
            2,
            50_000,
        ),
        (
            "command_timeout_secs",
            |b| b.command_timeout_secs,
            1,
            u64::MAX,
        ),
        (
            "command_max_output_len",
            |b| b.command_max_output_len,
            1024,
            131_072,
        ),
        ("max_tokens", |b| b.max_tokens, 256, 32_768),
        ("api_timeout_secs", |b| b.api_timeout_secs, 1, u64::MAX),
        ("api_max_retries", |b| b.api_max_retries, 0, 10),
        (
            "api_retry_delay_secs",
            |b| b.api_retry_delay_secs,
            1,
            u64::MAX,
        ),
        (
            "weather_timeout_secs",
            |b| b.weather_timeout_secs,
            1,
            u64::MAX,
        ),
        (
            "web_search_timeout_secs",
            |b| b.web_search_timeout_secs,
            1,
            u64::MAX,
        ),
        (
            "web_search_max_results",
            |b| b.web_search_max_results,
            1,
            20,
        ),
        (
            "http_fetch_timeout_secs",
            |b| b.http_fetch_timeout_secs,
            1,
            u64::MAX,
        ),
        (
            "http_fetch_max_response_bytes",
            |b| b.http_fetch_max_response_bytes,
            1024,
            4_194_304,
        ),
        (
            "reflection_default_max_rounds",
            |b| b.reflection_default_max_rounds,
            1,
            u64::MAX,
        ),
        (
            "plan_rewrite_max_attempts",
            |b| b.plan_rewrite_max_attempts,
            1,
            20,
        ),
        (
            "final_plan_semantic_check_max_non_readonly_tools",
            |b| b.final_plan_semantic_check_max_non_readonly_tools,
            0,
            32,
        ),
        (
            "final_plan_semantic_check_max_tokens",
            |b| b.final_plan_semantic_check_max_tokens,
            32,
            1024,
        ),
        (
            "cursor_rules_max_chars",
            |b| b.cursor_rules_max_chars,
            1024,
            1_000_000,
        ),
        ("skills_max_chars", |b| b.skills_max_chars, 1024, 1_000_000),
        ("skills_top_k", |b| b.skills_top_k, 1, 64),
        (
            "tool_message_max_chars",
            |b| b.tool_message_max_chars,
            1024,
            1_048_576,
        ),
        (
            "agent_tool_stats_window_events",
            |b| b.agent_tool_stats_window_events,
            16,
            65_536,
        ),
        (
            "agent_tool_stats_min_samples",
            |b| b.agent_tool_stats_min_samples,
            1,
            10_000,
        ),
        (
            "agent_tool_stats_max_chars",
            |b| b.agent_tool_stats_max_chars,
            64,
            32_768,
        ),
        (
            "context_char_budget",
            |b| b.context_char_budget,
            0,
            50_000_000,
        ),
        (
            "context_min_messages_after_system",
            |b| b.context_min_messages_after_system,
            1,
            128,
        ),
        (
            "context_summary_trigger_chars",
            |b| b.context_summary_trigger_chars,
            0,
            50_000_000,
        ),
        (
            "context_summary_tail_messages",
            |b| b.context_summary_tail_messages,
            4,
            64,
        ),
        (
            "context_summary_max_tokens",
            |b| b.context_summary_max_tokens,
            256,
            8192,
        ),
        (
            "context_summary_transcript_max_chars",
            |b| b.context_summary_transcript_max_chars,
            10_000,
            2_000_000,
        ),
        (
            "health_llm_models_probe_cache_secs",
            |b| b.health_llm_models_probe_cache_secs,
            5,
            86_400,
        ),
        (
            "chat_queue_max_concurrent",
            |b| b.chat_queue_max_concurrent,
            1,
            256,
        ),
        (
            "chat_queue_max_pending",
            |b| b.chat_queue_max_pending,
            1,
            8192,
        ),
        (
            "parallel_readonly_tools_max",
            |b| b.parallel_readonly_tools_max,
            1,
            256,
        ),
        (
            "read_file_turn_cache_max_entries",
            |b| b.read_file_turn_cache_max_entries,
            0,
            4096,
        ),
        (
            "test_result_cache_max_entries",
            |b| b.test_result_cache_max_entries,
            1,
            512,
        ),
        (
            "session_workspace_changelist_max_chars",
            |b| b.session_workspace_changelist_max_chars,
            0,
            500_000,
        ),
        (
            "staged_plan_patch_max_attempts",
            |b| b.staged_plan_patch_max_attempts,
            1,
            16,
        ),
        (
            "staged_plan_ensemble_count",
            |b| b.staged_plan_ensemble_count,
            1,
            3,
        ),
        (
            "sync_default_tool_sandbox_docker_timeout_secs",
            |b| b.sync_default_tool_sandbox_docker_timeout_secs,
            1,
            u64::MAX,
        ),
        (
            "agent_memory_file_max_chars",
            |b| b.agent_memory_file_max_chars,
            256,
            500_000,
        ),
        (
            "living_docs_inject_max_chars",
            |b| b.living_docs_inject_max_chars,
            0,
            500_000,
        ),
        (
            "living_docs_file_max_each_chars",
            |b| b.living_docs_file_max_each_chars,
            0,
            500_000,
        ),
        (
            "project_profile_inject_max_chars",
            |b| b.project_profile_inject_max_chars,
            0,
            500_000,
        ),
        (
            "project_dependency_brief_inject_max_chars",
            |b| b.project_dependency_brief_inject_max_chars,
            0,
            500_000,
        ),
        (
            "tool_call_explain_min_chars",
            |b| b.tool_call_explain_min_chars,
            1,
            256,
        ),
        (
            "tool_call_explain_max_chars",
            |b| b.tool_call_explain_max_chars,
            1,
            4000,
        ),
        (
            "long_term_memory_max_entries",
            |b| b.long_term_memory_max_entries,
            1,
            100_000,
        ),
        (
            "long_term_memory_inject_max_chars",
            |b| b.long_term_memory_inject_max_chars,
            256,
            500_000,
        ),
        (
            "long_term_memory_top_k",
            |b| b.long_term_memory_top_k,
            1,
            64,
        ),
        (
            "long_term_memory_max_chars_per_chunk",
            |b| b.long_term_memory_max_chars_per_chunk,
            256,
            32_000,
        ),
        (
            "long_term_memory_min_chars_to_index",
            |b| b.long_term_memory_min_chars_to_index,
            0,
            4096,
        ),
        (
            "long_term_memory_default_ttl_secs",
            |b| b.long_term_memory_default_ttl_secs,
            0,
            365 * 86400 * 10,
        ),
        (
            "mcp_tool_timeout_secs",
            |b| b.mcp_tool_timeout_secs,
            1,
            u64::MAX,
        ),
        (
            "codebase_semantic_max_file_bytes",
            |b| b.codebase_semantic_max_file_bytes,
            4096,
            4 * 1024 * 1024,
        ),
        (
            "codebase_semantic_chunk_max_chars",
            |b| b.codebase_semantic_chunk_max_chars,
            256,
            16_000,
        ),
        (
            "codebase_semantic_top_k",
            |b| b.codebase_semantic_top_k,
            1,
            64,
        ),
        (
            "codebase_semantic_query_max_chunks",
            |b| b.codebase_semantic_query_max_chunks,
            0,
            2_000_000,
        ),
        (
            "codebase_semantic_rebuild_max_files",
            |b| b.codebase_semantic_rebuild_max_files,
            1,
            100_000,
        ),
        (
            "codebase_semantic_fts_top_n",
            |b| b.codebase_semantic_fts_top_n,
            1,
            10_000,
        ),
        (
            "codebase_semantic_hybrid_semantic_pool",
            |b| b.codebase_semantic_hybrid_semantic_pool,
            1,
            10_000,
        ),
        (
            "tool_registry_http_fetch_wall_timeout_secs",
            |b| b.tool_registry_http_fetch_wall_timeout_secs,
            1,
            86_400,
        ),
        (
            "tool_registry_http_request_wall_timeout_secs",
            |b| b.tool_registry_http_request_wall_timeout_secs,
            1,
            86_400,
        ),
    ];
    for &(key, get, lo, hi) in ROWS {
        check_u64_inclusive(key, get(b), lo, hi)?;
    }
    Ok(())
}

fn validate_f64_table(b: &ConfigBuilder) -> Result<(), String> {
    const ROWS: &[F64RangeRow] = &[
        ("temperature", |b| b.temperature, 0.0, 2.0),
        (
            "agent_tool_stats_warn_below_success_ratio",
            |b| b.agent_tool_stats_warn_below_success_ratio,
            0.0,
            1.0,
        ),
        (
            "codebase_semantic_hybrid_alpha",
            |b| b.codebase_semantic_hybrid_alpha,
            0.0,
            1.0,
        ),
    ];
    for &(key, get, lo, hi) in ROWS {
        check_f64_inclusive(key, get(b), lo, hi)?;
    }
    Ok(())
}

/// 对 `finalize` 中会 `clamp` 的字段做前置校验；`None` 表示使用默认，跳过。
pub(super) fn validate_builder_numeric_ranges(b: &ConfigBuilder) -> Result<(), String> {
    validate_u64_table(b)?;
    check_i64_inclusive("llm_seed", b.llm_seed, i64::MIN, i64::MAX)?;
    validate_f64_table(b)?;

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
