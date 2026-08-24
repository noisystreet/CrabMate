//! 从 `CM_*` 环境变量覆盖 [`super::builder::ConfigBuilder`]（优先级高于磁盘 TOML）。

use super::builder::ConfigBuilder;
use super::env_override_apply::{
    apply_bool, apply_csv_allow_empty, apply_csv_nonempty, apply_nonempty_opt,
    apply_nonempty_opt_clearing_file, apply_nonempty_string, apply_nonempty_string_clearing_opt,
    apply_parse, apply_raw_opt, apply_trimmed_opt, env_flag_true,
};

#[path = "env_overrides_chat_queue.rs"]
mod env_overrides_chat_queue;
#[path = "env_overrides_mcp_codebase.rs"]
mod env_overrides_mcp_codebase;
#[path = "env_overrides_part9.rs"]
mod env_overrides_part9;
#[path = "env_overrides_per_plan_policy.rs"]
mod env_overrides_per_plan_policy;

/// 从 `CM_*` 环境变量覆盖 `ConfigBuilder` 字段。
pub(super) fn apply_env_overrides(b: &mut ConfigBuilder) {
    apply_env_overrides_part_1(b);
    apply_env_overrides_part_2(b);
    apply_env_overrides_part_3(b);
    apply_env_overrides_part_4(b);
    apply_env_overrides_part_5(b);
    apply_env_overrides_part_6(b);
    apply_env_overrides_part_7(b);
    apply_env_sync_tool_sandbox_overrides_part_8(b);
    apply_env_overrides_part_9(b);
    apply_env_overrides_part_10(b);
    apply_env_overrides_part_11(b);
    apply_env_overrides_part_12(b);
    apply_env_overrides_part_13(b);
    env_overrides_mcp_codebase::apply_env_overrides_part_14(b);
    apply_env_overrides_part_15(b);
}

fn apply_env_overrides_part_1(b: &mut ConfigBuilder) {
    env_override_api_base_models_auth(b);
    env_override_session_ui_flags(b);
    env_override_run_command_limits(b);
}

fn env_override_api_base_models_auth(b: &mut ConfigBuilder) {
    apply_nonempty_string(&mut b.llm.api_base, "CM_API_BASE");
    apply_nonempty_string(&mut b.llm.model, "CM_MODEL");
    apply_nonempty_opt(&mut b.llm.planner_model, "CM_PLANNER_MODEL");
    apply_nonempty_opt(&mut b.llm.executor_model, "CM_EXECUTOR_MODEL");
    apply_nonempty_opt(&mut b.llm.llm_http_auth_mode_str, "CM_LLM_HTTP_AUTH_MODE");
}

fn env_override_session_ui_flags(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.session_ui.max_message_history,
        "CM_MAX_MESSAGE_HISTORY",
    );
}

fn env_override_run_command_limits(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.command_exec.command_timeout_secs,
        "CM_COMMAND_TIMEOUT_SECS",
    );
    apply_parse(
        &mut b.command_exec.command_max_output_len,
        "CM_COMMAND_MAX_OUTPUT_LEN",
    );
    apply_csv_nonempty(&mut b.command_exec.allowed_commands, "CM_ALLOWED_COMMANDS");
    apply_nonempty_opt(
        &mut b.command_exec.run_command_working_dir,
        "CM_RUN_COMMAND_WORKING_DIR",
    );
}

fn apply_env_overrides_part_2(b: &mut ConfigBuilder) {
    env_override_workspace_allowed_roots(b);
    env_override_max_tokens_llm_numeric(b);
    env_override_llm_bool_flags(b);
    env_override_api_and_weather_timeouts(b);
    env_override_web_search_keys(b);
}

fn env_override_workspace_allowed_roots(b: &mut ConfigBuilder) {
    apply_csv_nonempty(
        &mut b.workspace_roots.workspace_allowed_roots,
        "CM_WORKSPACE_ALLOWED_ROOTS",
    );
    apply_nonempty_opt(
        &mut b.workspace_roots.web_workspace_pool,
        "CM_WEB_WORKSPACE_POOL",
    );
}

fn env_override_max_tokens_llm_numeric(b: &mut ConfigBuilder) {
    apply_parse(&mut b.llm_sampling.max_tokens, "CM_MAX_TOKENS");
    apply_parse(
        &mut b.llm_sampling.llm_context_tokens,
        "CM_LLM_CONTEXT_TOKENS",
    );
    apply_parse(&mut b.llm_sampling.temperature, "CM_TEMPERATURE");
    apply_parse(&mut b.llm_sampling.llm_seed, "CM_LLM_SEED");
}

fn env_override_llm_bool_flags(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.llm_vendor.llm_reasoning_split,
        "CM_LLM_REASONING_SPLIT",
    );
    apply_bool(
        &mut b.llm_vendor.llm_bigmodel_thinking,
        "CM_LLM_BIGMODEL_THINKING",
    );
    apply_bool(
        &mut b.llm_vendor.llm_kimi_thinking_disabled,
        "CM_LLM_KIMI_THINKING_DISABLED",
    );
}

fn env_override_api_and_weather_timeouts(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.llm_http_retry.api_timeout_secs,
        "CM_API_TIMEOUT_SECS",
    );
    apply_parse(&mut b.llm_http_retry.api_max_retries, "CM_API_MAX_RETRIES");
    apply_parse(
        &mut b.llm_http_retry.api_retry_delay_secs,
        "CM_API_RETRY_DELAY_SECS",
    );
    apply_parse(
        &mut b.weather_tool.weather_timeout_secs,
        "CM_WEATHER_TIMEOUT_SECS",
    );
}

fn env_override_web_search_keys(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.web_search.web_search_provider_str,
        "CM_WEB_SEARCH_PROVIDER",
    );
    apply_raw_opt(
        &mut b.web_search.web_search_api_key,
        "CM_WEB_SEARCH_API_KEY",
    );
}

fn apply_env_overrides_part_3(b: &mut ConfigBuilder) {
    env_override_web_search_limits_part_3(b);
    env_override_http_fetch_limits(b);
    env_overrides_per_plan_policy::env_override_reflection_and_final_plan(b);
}

fn env_override_web_search_limits_part_3(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.web_search.web_search_timeout_secs,
        "CM_WEB_SEARCH_TIMEOUT_SECS",
    );
    apply_parse(
        &mut b.web_search.web_search_max_results,
        "CM_WEB_SEARCH_MAX_RESULTS",
    );
}

fn env_override_http_fetch_limits(b: &mut ConfigBuilder) {
    apply_csv_allow_empty(
        &mut b.http_fetch.http_fetch_allowed_prefixes,
        "CM_HTTP_FETCH_ALLOWED_PREFIXES",
    );
    apply_parse(
        &mut b.http_fetch.http_fetch_timeout_secs,
        "CM_HTTP_FETCH_TIMEOUT_SECS",
    );
    apply_parse(
        &mut b.http_fetch.http_fetch_max_response_bytes,
        "CM_HTTP_FETCH_MAX_RESPONSE_BYTES",
    );
    apply_nonempty_opt(
        &mut b.http_fetch.http_fetch_user_agent,
        "CM_HTTP_FETCH_USER_AGENT",
    );
}

fn apply_env_overrides_part_4(b: &mut ConfigBuilder) {
    env_override_system_prompt_and_default_role(b);
}

fn env_override_system_prompt_and_default_role(b: &mut ConfigBuilder) {
    apply_nonempty_string_clearing_opt(
        &mut b.roles_prompts.system_prompt,
        &mut b.roles_prompts.system_prompt_file,
        "CM_SYSTEM_PROMPT",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.system_prompt_file,
        "CM_SYSTEM_PROMPT_FILE",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.default_agent_role_id,
        "CM_DEFAULT_CM_ROLE",
    );
    apply_bool(
        &mut b.roles_prompts.coding_workbench_enabled,
        "CM_CODING_WORKBENCH_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.coding_workbench_increment_file,
        "CM_CODING_WORKBENCH_INCREMENT_FILE",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.default_session_mode,
        "CM_DEFAULT_SESSION_MODE",
    );
}

fn apply_env_overrides_part_5(b: &mut ConfigBuilder) {
    env_override_cursor_rules_part_5(b);
    env_override_skills_part_5(b);
    env_override_tool_streaming_flags(b);
}

fn env_override_cursor_rules_part_5(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.cursor_rules.cursor_rules_enabled,
        "CM_CURSOR_RULES_ENABLED",
    );
    apply_nonempty_opt(&mut b.cursor_rules.cursor_rules_dir, "CM_CURSOR_RULES_DIR");
    apply_bool(
        &mut b.cursor_rules.cursor_rules_include_agents_md,
        "CM_CURSOR_RULES_INCLUDE_AGENTS_MD",
    );
    apply_parse(
        &mut b.cursor_rules.cursor_rules_max_chars,
        "CM_CURSOR_RULES_MAX_CHARS",
    );
}

fn env_override_skills_part_5(b: &mut ConfigBuilder) {
    apply_bool(&mut b.skills.skills_enabled, "CM_SKILLS_ENABLED");
    apply_nonempty_opt(&mut b.skills.skills_dir, "CM_SKILLS_DIR");
    // `CM_SKILLS_DISABLE_HOST_LAYERS=1`：测试/CI 一次关掉用户+系统层（仍可用下面两个变量单独覆盖）。
    if env_flag_true("CM_SKILLS_DISABLE_HOST_LAYERS") {
        b.skills.skills_user_dir = Some(String::new());
        b.skills.skills_system_dir = Some(String::new());
    }
    apply_trimmed_opt(&mut b.skills.skills_user_dir, "CM_SKILLS_USER_DIR");
    apply_trimmed_opt(&mut b.skills.skills_system_dir, "CM_SKILLS_SYSTEM_DIR");
    apply_parse(&mut b.skills.skills_max_chars, "CM_SKILLS_MAX_CHARS");
    apply_parse(&mut b.skills.skills_top_k, "CM_SKILLS_TOP_K");
}

fn env_override_tool_streaming_flags(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.tool_transcript.tool_message_max_chars,
        "CM_TOOL_MESSAGE_MAX_CHARS",
    );
    apply_bool(
        &mut b.tool_transcript.sse_tool_call_include_arguments,
        "CM_SSE_TOOL_CALL_INCLUDE_ARGUMENTS",
    );
    apply_bool(
        &mut b.agent_thinking_trace.agent_thinking_trace_enabled,
        "CM_THINKING_TRACE_ENABLED",
    );
    apply_bool(
        &mut b.tool_transcript.tool_result_envelope_v1,
        "CM_TOOL_RESULT_ENVELOPE_V1",
    );
    apply_bool(
        &mut b.agent_tool_stats.agent_tool_stats_enabled,
        "CM_TOOL_STATS_ENABLED",
    );
}

fn apply_env_overrides_part_6(b: &mut ConfigBuilder) {
    env_override_tool_stats_numeric(b);
    env_override_thinking_echo_appendix(b);
    env_override_context_budget_and_summary(b);
}

fn env_override_tool_stats_numeric(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_window_events,
        "CM_TOOL_STATS_WINDOW_EVENTS",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_min_samples,
        "CM_TOOL_STATS_MIN_SAMPLES",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_max_chars,
        "CM_TOOL_STATS_MAX_CHARS",
    );
    apply_parse(
        &mut b.agent_tool_stats.agent_tool_stats_warn_below_success_ratio,
        "CM_TOOL_STATS_WARN_BELOW_SUCCESS_RATIO",
    );
}

fn env_override_thinking_echo_appendix(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.thinking_echo.thinking_avoid_echo_system_prompt,
        "CM_THINKING_AVOID_ECHO_SYSTEM_PROMPT",
    );
    // 与 CM_SYSTEM_PROMPT_FILE 一致：后处理覆盖，故同时设置时文件优先于内联。
    apply_nonempty_opt_clearing_file(
        &mut b.thinking_echo.thinking_avoid_echo_appendix,
        &mut b.thinking_echo.thinking_avoid_echo_appendix_file,
        "CM_THINKING_AVOID_ECHO_APPENDIX",
    );
    apply_nonempty_opt(
        &mut b.thinking_echo.thinking_avoid_echo_appendix_file,
        "CM_THINKING_AVOID_ECHO_APPENDIX_FILE",
    );
}

fn env_override_context_budget_and_summary(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.context_pipeline.context_char_budget,
        "CM_CONTEXT_CHAR_BUDGET",
    );
    apply_parse(
        &mut b.context_pipeline.context_min_messages_after_system,
        "CM_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM",
    );
    apply_parse(
        &mut b.context_pipeline.context_token_trigger_percent,
        "CM_CONTEXT_TOKEN_TRIGGER_PERCENT",
    );
    apply_parse(
        &mut b.context_pipeline.context_token_target_percent,
        "CM_CONTEXT_TOKEN_TARGET_PERCENT",
    );
    apply_parse(
        &mut b.context_pipeline.context_token_safety_margin_tokens,
        "CM_CONTEXT_TOKEN_SAFETY_MARGIN_TOKENS",
    );
    apply_parse(
        &mut b.context_pipeline.context_summary_trigger_chars,
        "CM_CONTEXT_SUMMARY_TRIGGER_CHARS",
    );
    apply_parse(
        &mut b.context_pipeline.context_summary_tail_messages,
        "CM_CONTEXT_SUMMARY_TAIL_MESSAGES",
    );
    apply_parse(
        &mut b.context_pipeline.context_summary_max_tokens,
        "CM_CONTEXT_SUMMARY_MAX_TOKENS",
    );
}

fn apply_env_overrides_part_7(b: &mut ConfigBuilder) {
    env_override_context_transcript_and_health_probe(b);
    env_overrides_chat_queue::env_override_chat_queue_parallel_and_caches(b);
}

fn env_override_context_transcript_and_health_probe(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.context_pipeline.context_summary_transcript_max_chars,
        "CM_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS",
    );
    apply_nonempty_opt(
        &mut b.context_pipeline.context_summary_system_file,
        "CM_CONTEXT_SUMMARY_SYSTEM_FILE",
    );
    apply_nonempty_opt(
        &mut b.context_pipeline.context_summary_user_file,
        "CM_CONTEXT_SUMMARY_USER_FILE",
    );
    apply_bool(
        &mut b.web_api.health_llm_models_probe,
        "CM_HEALTH_LLM_MODELS_PROBE",
    );
    apply_parse(
        &mut b.web_api.health_llm_models_probe_cache_secs,
        "CM_HEALTH_LLM_MODELS_PROBE_CACHE_SECS",
    );
}

fn apply_env_sync_tool_sandbox_overrides_part_8(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_mode_str,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_MODE",
    );
    apply_nonempty_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_docker_image,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE",
    );
    apply_raw_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_docker_network,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK",
    );
    apply_parse(
        &mut b
            .sync_tool_sandbox
            .sync_default_tool_sandbox_docker_timeout_secs,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS",
    );
}

fn apply_env_overrides_part_9(b: &mut ConfigBuilder) {
    env_overrides_part9::apply_env_overrides_part_9(b);
}

fn apply_env_overrides_part_10(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.context_bootstrap_inject.living_docs_inject_enabled,
        "CM_LIVING_DOCS_INJECT_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.context_bootstrap_inject.living_docs_relative_dir,
        "CM_LIVING_DOCS_RELATIVE_DIR",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.living_docs_inject_max_chars,
        "CM_LIVING_DOCS_INJECT_MAX_CHARS",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.living_docs_file_max_each_chars,
        "CM_LIVING_DOCS_FILE_MAX_EACH_CHARS",
    );
    apply_bool(
        &mut b.context_bootstrap_inject.project_profile_inject_enabled,
        "CM_PROJECT_PROFILE_INJECT_ENABLED",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.project_profile_inject_max_chars,
        "CM_PROJECT_PROFILE_INJECT_MAX_CHARS",
    );
}

fn apply_env_overrides_part_11(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b
            .context_bootstrap_inject
            .project_dependency_brief_inject_enabled,
        "CM_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED",
    );
    apply_parse(
        &mut b
            .context_bootstrap_inject
            .project_dependency_brief_inject_max_chars,
        "CM_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS",
    );
    apply_bool(
        &mut b.tool_call_explain.tool_call_explain_enabled,
        "CM_TOOL_CALL_EXPLAIN_ENABLED",
    );
    apply_parse(
        &mut b.tool_call_explain.tool_call_explain_min_chars,
        "CM_TOOL_CALL_EXPLAIN_MIN_CHARS",
    );
    apply_parse(
        &mut b.tool_call_explain.tool_call_explain_max_chars,
        "CM_TOOL_CALL_EXPLAIN_MAX_CHARS",
    );
}

fn apply_env_overrides_part_12(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.long_term_memory.long_term_memory_enabled,
        "CM_LONG_TERM_MEMORY_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.long_term_memory.long_term_memory_scope_mode_str,
        "CM_LONG_TERM_MEMORY_SCOPE_MODE",
    );
    apply_nonempty_opt(
        &mut b.long_term_memory.long_term_memory_vector_backend_str,
        "CM_LONG_TERM_MEMORY_VECTOR_BACKEND",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_max_entries,
        "CM_LONG_TERM_MEMORY_MAX_ENTRIES",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_inject_max_chars,
        "CM_LONG_TERM_MEMORY_INJECT_MAX_CHARS",
    );
    apply_nonempty_opt(
        &mut b.long_term_memory.long_term_memory_store_sqlite_path,
        "CM_LONG_TERM_MEMORY_STORE_SQLITE_PATH",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_top_k,
        "CM_LONG_TERM_MEMORY_TOP_K",
    );
}

fn apply_env_overrides_part_13(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.long_term_memory.long_term_memory_max_chars_per_chunk,
        "CM_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_min_chars_to_index,
        "CM_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX",
    );
    apply_bool(
        &mut b.long_term_memory.long_term_memory_async_index,
        "CM_LONG_TERM_MEMORY_ASYNC_INDEX",
    );
    apply_bool(
        &mut b.long_term_memory.long_term_memory_auto_index_turns,
        "CM_LONG_TERM_MEMORY_AUTO_INDEX_TURNS",
    );
    apply_bool(
        &mut b
            .long_term_memory
            .long_term_memory_auto_summarize_experience,
        "CM_LONG_TERM_MEMORY_AUTO_SUMMARIZE_EXPERIENCE",
    );
    apply_bool(
        &mut b
            .long_term_memory
            .long_term_memory_prioritize_experience_recall,
        "CM_LONG_TERM_MEMORY_PRIORITIZE_EXPERIENCE_RECALL",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_default_ttl_secs,
        "CM_LONG_TERM_MEMORY_DEFAULT_TTL_SECS",
    );
}

fn apply_env_overrides_part_15(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_top_k,
        "CM_CODEBASE_SEMANTIC_TOP_K",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_query_max_chunks,
        "CM_CODEBASE_SEMANTIC_QUERY_MAX_CHUNKS",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_rebuild_max_files,
        "CM_CODEBASE_SEMANTIC_REBUILD_MAX_FILES",
    );
    apply_bool(
        &mut b.codebase_semantic.codebase_semantic_rebuild_incremental,
        "CM_CODEBASE_SEMANTIC_REBUILD_INCREMENTAL",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_hybrid_alpha,
        "CM_CODEBASE_SEMANTIC_HYBRID_ALPHA",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_fts_top_n,
        "CM_CODEBASE_SEMANTIC_FTS_TOP_N",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_hybrid_semantic_pool,
        "CM_CODEBASE_SEMANTIC_HYBRID_SEMANTIC_POOL",
    );
}
