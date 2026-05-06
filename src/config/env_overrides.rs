//! 从 `CM_*` 环境变量覆盖 [`super::builder::ConfigBuilder`]（优先级高于磁盘 TOML）。

use super::builder::ConfigBuilder;
use super::source::parse_bool_like;

/// 从 `CM_*` 环境变量覆盖 `ConfigBuilder` 字段。
pub(super) fn apply_env_overrides(b: &mut ConfigBuilder) {
    apply_env_overrides_part_1(b);
    apply_env_overrides_part_2(b);
    apply_env_overrides_part_3(b);
    apply_env_overrides_part_4(b);
    apply_env_overrides_part_5(b);
    apply_env_overrides_part_6(b);
    apply_env_overrides_part_7(b);
    apply_env_overrides_part_8(b);
    apply_env_overrides_part_9(b);
    apply_env_overrides_part_10(b);
    apply_env_overrides_part_11(b);
    apply_env_overrides_part_12(b);
    apply_env_overrides_part_13(b);
    apply_env_overrides_part_14(b);
    apply_env_overrides_part_15(b);
}

fn apply_env_overrides_part_1(b: &mut ConfigBuilder) {
    env_override_api_base_models_auth(b);
    env_override_tui_repl_session_flags(b);
    env_override_run_command_limits(b);
}

fn env_override_api_base_models_auth(b: &mut ConfigBuilder) {
    if let Ok(a) = std::env::var("CM_API_BASE") {
        let a = a.trim().to_string();
        if !a.is_empty() {
            b.llm.api_base = a;
        }
    }
    if let Ok(m) = std::env::var("CM_MODEL") {
        let m = m.trim().to_string();
        if !m.is_empty() {
            b.llm.model = m;
        }
    }
    if let Ok(s) = std::env::var("CM_PLANNER_MODEL") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.llm.planner_model = Some(s);
        }
    }
    if let Ok(s) = std::env::var("CM_EXECUTOR_MODEL") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.llm.executor_model = Some(s);
        }
    }
    if let Ok(s) = std::env::var("CM_LLM_HTTP_AUTH_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.llm.llm_http_auth_mode_str = Some(s);
        }
    }
}

fn env_override_tui_repl_session_flags(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_MAX_MESSAGE_HISTORY")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.session_ui.max_message_history = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TUI_SESSION_MAX_MESSAGES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.session_ui.tui_session_max_messages = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TUI_LOAD_SESSION_ON_START")
        && let Some(val) = parse_bool_like(&v)
    {
        b.session_ui.tui_load_session_on_start = Some(val);
    }
    if let Ok(v) = std::env::var("CM_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.session_ui.repl_initial_workspace_messages_enabled = Some(val);
    }
}

fn env_override_run_command_limits(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_COMMAND_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.command_exec.command_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_COMMAND_MAX_OUTPUT_LEN")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.command_exec.command_max_output_len = Some(n);
    }
    if let Ok(v) = std::env::var("CM_ALLOWED_COMMANDS") {
        let list = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !list.is_empty() {
            b.command_exec.allowed_commands = Some(list);
        }
    }
    if let Ok(v) = std::env::var("CM_RUN_COMMAND_WORKING_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.command_exec.run_command_working_dir = Some(v);
        }
    }
}

fn apply_env_overrides_part_2(b: &mut ConfigBuilder) {
    env_override_workspace_allowed_roots(b);
    env_override_max_tokens_llm_numeric(b);
    env_override_llm_bool_flags(b);
    env_override_api_and_weather_timeouts(b);
    env_override_web_search_keys(b);
}

fn env_override_workspace_allowed_roots(b: &mut ConfigBuilder) {
    if let Ok(s) = std::env::var("CM_WORKSPACE_ALLOWED_ROOTS") {
        let list: Vec<String> = s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !list.is_empty() {
            b.workspace_roots.workspace_allowed_roots = Some(list);
        }
    }
}

fn env_override_max_tokens_llm_numeric(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.llm_sampling.max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LLM_CONTEXT_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.llm_sampling.llm_context_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TEMPERATURE")
        && let Ok(n) = v.trim().parse::<f64>()
    {
        b.llm_sampling.temperature = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LLM_SEED")
        && let Ok(n) = v.trim().parse::<i64>()
    {
        b.llm_sampling.llm_seed = Some(n);
    }
}

fn env_override_llm_bool_flags(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_LLM_REASONING_SPLIT")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_vendor.llm_reasoning_split = Some(val);
    }
    if let Ok(v) = std::env::var("CM_LLM_BIGMODEL_THINKING")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_vendor.llm_bigmodel_thinking = Some(val);
    }
    if let Ok(v) = std::env::var("CM_LLM_KIMI_THINKING_DISABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_vendor.llm_kimi_thinking_disabled = Some(val);
    }
}

fn env_override_api_and_weather_timeouts(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_API_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.llm_http_retry.api_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_API_MAX_RETRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.llm_http_retry.api_max_retries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_API_RETRY_DELAY_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.llm_http_retry.api_retry_delay_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_WEATHER_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.weather_tool.weather_timeout_secs = Some(n);
    }
}

fn env_override_web_search_keys(b: &mut ConfigBuilder) {
    if let Ok(s) = std::env::var("CM_WEB_SEARCH_PROVIDER") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.web_search.web_search_provider_str = Some(s);
        }
    }
    if let Ok(k) = std::env::var("CM_WEB_SEARCH_API_KEY") {
        b.web_search.web_search_api_key = Some(k);
    }
}

fn apply_env_overrides_part_3(b: &mut ConfigBuilder) {
    env_override_web_search_limits_part_3(b);
    env_override_http_fetch_limits(b);
    env_override_reflection_and_final_plan(b);
}

fn env_override_web_search_limits_part_3(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_WEB_SEARCH_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.web_search.web_search_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_WEB_SEARCH_MAX_RESULTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.web_search.web_search_max_results = Some(n);
    }
}

fn env_override_http_fetch_limits(b: &mut ConfigBuilder) {
    if let Ok(s) = std::env::var("CM_HTTP_FETCH_ALLOWED_PREFIXES") {
        let list: Vec<String> = s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !list.is_empty() {
            b.http_fetch.http_fetch_allowed_prefixes = Some(list);
        }
    }
    if let Ok(v) = std::env::var("CM_HTTP_FETCH_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.http_fetch.http_fetch_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_HTTP_FETCH_MAX_RESPONSE_BYTES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.http_fetch.http_fetch_max_response_bytes = Some(n);
    }
}

fn env_override_reflection_and_final_plan(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_REFLECTION_DEFAULT_MAX_ROUNDS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy.reflection_default_max_rounds = Some(n);
    }
    if let Ok(s) = std::env::var("CM_FINAL_PLAN_REQUIREMENT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.per_plan_policy.final_plan_requirement_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("CM_PLAN_REWRITE_MAX_ATTEMPTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy.plan_rewrite_max_attempts = Some(n);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_REQUIRE_STRICT_WORKFLOW_NODE_COVERAGE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.per_plan_policy
            .final_plan_require_strict_workflow_node_coverage = Some(val);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_SEMANTIC_CHECK_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.per_plan_policy.final_plan_semantic_check_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_NON_READONLY_TOOLS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy
            .final_plan_semantic_check_max_non_readonly_tools = Some(n);
    }
    if let Ok(v) = std::env::var("CM_FINAL_PLAN_SEMANTIC_CHECK_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.per_plan_policy.final_plan_semantic_check_max_tokens = Some(n);
    }
    if let Ok(s) = std::env::var("CM_PLANNER_EXECUTOR_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.per_plan_policy.planner_executor_mode_str = Some(s);
        }
    }
}

fn apply_env_overrides_part_4(b: &mut ConfigBuilder) {
    env_override_intent_thresholds(b);
    env_override_system_prompt_and_default_role(b);
}

fn env_override_intent_thresholds(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_INTENT_AT_TURN_START_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_at_turn_start_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_INTENT_L2_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_l2_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_INTENT_L2_MIN_CONFIDENCE")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_l2_min_confidence = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_L2_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.intent_routing.intent_l2_max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("CM_INTENT_L0_ROUTING_BOOST_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_l0_routing_boost_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_INTENT_EXECUTE_LOW_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_execute_low_threshold = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_EXECUTE_HIGH_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_execute_high_threshold = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_NON_HIER_EXECUTE_LOW_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_non_hier_execute_low_threshold = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_NON_HIER_EXECUTE_HIGH_THRESHOLD")
        && let Ok(f) = v.trim().parse::<f64>()
    {
        b.intent_routing.intent_non_hier_execute_high_threshold = Some(f);
    }
    if let Ok(v) = std::env::var("CM_INTENT_MODE_BIAS_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.intent_routing.intent_mode_bias_enabled = Some(val);
    }
}

fn env_override_system_prompt_and_default_role(b: &mut ConfigBuilder) {
    if let Ok(s) = std::env::var("CM_SYSTEM_PROMPT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.roles_prompts.system_prompt = s;
            b.roles_prompts.system_prompt_file = None;
        }
    }
    if let Ok(p) = std::env::var("CM_SYSTEM_PROMPT_FILE") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            b.roles_prompts.system_prompt_file = Some(p);
        }
    }
    if let Ok(s) = std::env::var("CM_DEFAULT_CM_ROLE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.roles_prompts.default_agent_role_id = Some(s);
        }
    }
}

fn apply_env_overrides_part_5(b: &mut ConfigBuilder) {
    env_override_cursor_rules_part_5(b);
    env_override_skills_part_5(b);
    env_override_tool_streaming_flags(b);
}

fn env_override_cursor_rules_part_5(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CURSOR_RULES_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.cursor_rules.cursor_rules_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CURSOR_RULES_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.cursor_rules.cursor_rules_dir = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_CURSOR_RULES_INCLUDE_AGENTS_MD")
        && let Some(val) = parse_bool_like(&v)
    {
        b.cursor_rules.cursor_rules_include_agents_md = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CURSOR_RULES_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.cursor_rules.cursor_rules_max_chars = Some(n);
    }
}

fn env_override_skills_part_5(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_SKILLS_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.skills.skills_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_SKILLS_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.skills.skills_dir = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_SKILLS_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.skills.skills_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_SKILLS_TOP_K")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.skills.skills_top_k = Some(n);
    }
}

fn env_override_tool_streaming_flags(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_TOOL_MESSAGE_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tool_transcript.tool_message_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_SSE_TOOL_CALL_INCLUDE_ARGUMENTS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.tool_transcript.sse_tool_call_include_arguments = Some(val);
    }
    if let Ok(v) = std::env::var("CM_THINKING_TRACE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.agent_thinking_trace.agent_thinking_trace_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_TOOL_RESULT_ENVELOPE_V1")
        && let Some(val) = parse_bool_like(&v)
    {
        b.tool_transcript.tool_result_envelope_v1 = Some(val);
    }
    if let Ok(v) = std::env::var("CM_TOOL_STATS_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.agent_tool_stats.agent_tool_stats_enabled = Some(val);
    }
}

fn apply_env_overrides_part_6(b: &mut ConfigBuilder) {
    env_override_tool_stats_numeric(b);
    env_override_thinking_echo_appendix(b);
    env_override_context_budget_and_summary(b);
}

fn env_override_tool_stats_numeric(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_TOOL_STATS_WINDOW_EVENTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.agent_tool_stats.agent_tool_stats_window_events = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TOOL_STATS_MIN_SAMPLES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.agent_tool_stats.agent_tool_stats_min_samples = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TOOL_STATS_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.agent_tool_stats.agent_tool_stats_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TOOL_STATS_WARN_BELOW_SUCCESS_RATIO")
        && let Ok(x) = v.trim().parse::<f64>()
    {
        b.agent_tool_stats.agent_tool_stats_warn_below_success_ratio = Some(x);
    }
}

fn env_override_thinking_echo_appendix(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.dsml_materialize.materialize_deepseek_dsml_tool_calls = Some(val);
    }
    if let Ok(v) = std::env::var("CM_THINKING_AVOID_ECHO_SYSTEM_PROMPT")
        && let Some(val) = parse_bool_like(&v)
    {
        b.thinking_echo.thinking_avoid_echo_system_prompt = Some(val);
    }
    if let Ok(s) = std::env::var("CM_THINKING_AVOID_ECHO_APPENDIX") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.thinking_echo.thinking_avoid_echo_appendix = Some(s);
            b.thinking_echo.thinking_avoid_echo_appendix_file = None;
        }
    }
    // 与 CM_SYSTEM_PROMPT_FILE 一致：后处理覆盖，故同时设置时文件优先于内联。
    if let Ok(p) = std::env::var("CM_THINKING_AVOID_ECHO_APPENDIX_FILE") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            b.thinking_echo.thinking_avoid_echo_appendix_file = Some(p);
        }
    }
}

fn env_override_context_budget_and_summary(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CONTEXT_CHAR_BUDGET")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_pipeline.context_char_budget = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_pipeline.context_min_messages_after_system = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CONTEXT_SUMMARY_TRIGGER_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_pipeline.context_summary_trigger_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CONTEXT_SUMMARY_TAIL_MESSAGES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_pipeline.context_summary_tail_messages = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CONTEXT_SUMMARY_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_pipeline.context_summary_max_tokens = Some(n);
    }
}

fn apply_env_overrides_part_7(b: &mut ConfigBuilder) {
    env_override_context_transcript_and_health_probe(b);
    env_override_chat_queue_parallel_and_caches(b);
    env_override_staged_plan_execution_flags(b);
}

fn env_override_context_transcript_and_health_probe(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_pipeline.context_summary_transcript_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_HEALTH_LLM_MODELS_PROBE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.health_llm_models_probe = Some(val);
    }
    if let Ok(v) = std::env::var("CM_HEALTH_LLM_MODELS_PROBE_CACHE_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.web_api.health_llm_models_probe_cache_secs = Some(n);
    }
}

fn env_override_chat_queue_parallel_and_caches(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CHAT_QUEUE_MAX_CONCURRENT")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.chat_queue_max_concurrent = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CHAT_QUEUE_MAX_PENDING")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.chat_queue_max_pending = Some(n);
    }
    if let Ok(v) = std::env::var("CM_PARALLEL_READONLY_TOOLS_MAX")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.parallel_readonly_tools_max = Some(n);
    }
    if let Ok(v) = std::env::var("CM_READ_FILE_TURN_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.read_file_turn_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_READONLY_TOOL_TTL_CACHE_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.readonly_tool_ttl_cache_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_READONLY_TOOL_TTL_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.readonly_tool_ttl_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TEST_RESULT_CACHE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.chat_queues_cache.test_result_cache_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_TEST_RESULT_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.test_result_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_SESSION_WORKSPACE_CHANGELIST_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.session_workspace_changelist
            .session_workspace_changelist_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.session_workspace_changelist
            .session_workspace_changelist_max_chars = Some(n);
    }
}

fn env_override_staged_plan_execution_flags(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_EXECUTION")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning.staged_plan_execution = Some(val);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_ALLOW_NO_TASK")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning.staged_plan_allow_no_task = Some(val);
    }
}

fn apply_env_overrides_part_8(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_PHASE_INSTRUCTION") {
        b.staged_planning.staged_plan_phase_instruction = Some(v);
    }
    if let Ok(s) = std::env::var("CM_STAGED_PLAN_FEEDBACK_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.staged_planning.staged_plan_feedback_mode_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_PATCH_MAX_ATTEMPTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.staged_planning.staged_plan_patch_max_attempts = Some(n);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning.staged_plan_cli_show_planner_stream = Some(val);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_OPTIMIZER_ROUND")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning.staged_plan_optimizer_round = Some(val);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_OPTIMIZER_REQUIRES_PARALLEL_TOOLS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning
            .staged_plan_optimizer_requires_parallel_tools = Some(val);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_ENSEMBLE_COUNT")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.staged_planning.staged_plan_ensemble_count = Some(n);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_SKIP_ENSEMBLE_ON_CASUAL_PROMPT")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning.staged_plan_skip_ensemble_on_casual_prompt = Some(val);
    }
    if let Ok(v) = std::env::var("CM_STAGED_PLAN_TWO_PHASE_NL_DISPLAY")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_planning.staged_plan_two_phase_nl_display = Some(val);
    }
    if let Ok(s) = std::env::var("CM_SYNC_DEFAULT_TOOL_SANDBOX_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.sync_tool_sandbox.sync_default_tool_sandbox_mode_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.sync_tool_sandbox.sync_default_tool_sandbox_docker_image = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK") {
        b.sync_tool_sandbox.sync_default_tool_sandbox_docker_network = Some(v);
    }
    if let Ok(v) = std::env::var("CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.sync_tool_sandbox
            .sync_default_tool_sandbox_docker_timeout_secs = Some(n);
    }
}

fn apply_env_overrides_part_9(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.sync_tool_sandbox.sync_default_tool_sandbox_docker_user = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_WEB_API_BEARER_TOKEN") {
        b.web_api.web_api_bearer_token = Some(v.trim().to_string());
    }
    if let Ok(v) = std::env::var("CM_WEB_API_REQUIRE_BEARER")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.web_api_require_bearer = Some(val);
    }
    if let Ok(v) = std::env::var("CM_WEB_AUDIT_LOG_WRITE_TOOLS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.web_audit_log_write_tools = Some(val);
    }
    if let Ok(v) = std::env::var("CM_WEB_AUDIT_TRUST_X_FORWARDED_FOR")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.web_audit_trust_x_forwarded_for = Some(val);
    }
    if let Ok(v) = std::env::var("CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.allow_insecure_no_auth_for_non_loopback = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CONVERSATION_STORE_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.conversation_persistence.conversation_store_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_MEMORY_FILE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.context_bootstrap_inject.agent_memory_file_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_MEMORY_FILE") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.context_bootstrap_inject.agent_memory_file = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_MEMORY_FILE_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_bootstrap_inject.agent_memory_file_max_chars = Some(n);
    }
}

fn apply_env_overrides_part_10(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_LIVING_DOCS_INJECT_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.context_bootstrap_inject.living_docs_inject_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_LIVING_DOCS_RELATIVE_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.context_bootstrap_inject.living_docs_relative_dir = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_LIVING_DOCS_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_bootstrap_inject.living_docs_inject_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LIVING_DOCS_FILE_MAX_EACH_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_bootstrap_inject.living_docs_file_max_each_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_PROJECT_PROFILE_INJECT_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.context_bootstrap_inject.project_profile_inject_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_PROJECT_PROFILE_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_bootstrap_inject.project_profile_inject_max_chars = Some(n);
    }
}

fn apply_env_overrides_part_11(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.context_bootstrap_inject
            .project_dependency_brief_inject_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_bootstrap_inject
            .project_dependency_brief_inject_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TOOL_CALL_EXPLAIN_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.tool_call_explain.tool_call_explain_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_TOOL_CALL_EXPLAIN_MIN_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tool_call_explain.tool_call_explain_min_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TOOL_CALL_EXPLAIN_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tool_call_explain.tool_call_explain_max_chars = Some(n);
    }
}

fn apply_env_overrides_part_12(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.long_term_memory.long_term_memory_enabled = Some(val);
    }
    if let Ok(s) = std::env::var("CM_LONG_TERM_MEMORY_SCOPE_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.long_term_memory.long_term_memory_scope_mode_str = Some(s);
        }
    }
    if let Ok(s) = std::env::var("CM_LONG_TERM_MEMORY_VECTOR_BACKEND") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.long_term_memory.long_term_memory_vector_backend_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory.long_term_memory_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory.long_term_memory_inject_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_STORE_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.long_term_memory.long_term_memory_store_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_TOP_K")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory.long_term_memory_top_k = Some(n);
    }
}

fn apply_env_overrides_part_13(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory.long_term_memory_max_chars_per_chunk = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory.long_term_memory_min_chars_to_index = Some(n);
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_ASYNC_INDEX")
        && let Some(val) = parse_bool_like(&v)
    {
        b.long_term_memory.long_term_memory_async_index = Some(val);
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_AUTO_INDEX_TURNS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.long_term_memory.long_term_memory_auto_index_turns = Some(val);
    }
    if let Ok(v) = std::env::var("CM_LONG_TERM_MEMORY_DEFAULT_TTL_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory.long_term_memory_default_ttl_secs = Some(n);
    }
}

fn apply_env_overrides_part_14(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_MCP_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.mcp_client.mcp_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_MCP_COMMAND") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.mcp_client.mcp_command = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_MCP_TOOL_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.mcp_client.mcp_tool_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_SEARCH_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic.codebase_semantic_search_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_INVALIDATE_ON_WORKSPACE_CHANGE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.codebase_semantic.codebase_semantic_index_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_MAX_FILE_BYTES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_max_file_bytes = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_chunk_max_chars = Some(n);
    }
}

fn apply_env_overrides_part_15(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_TOP_K")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_top_k = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_QUERY_MAX_CHUNKS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_query_max_chunks = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_REBUILD_MAX_FILES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_rebuild_max_files = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_REBUILD_INCREMENTAL")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic.codebase_semantic_rebuild_incremental = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_HYBRID_ALPHA")
        && let Ok(a) = v.trim().parse::<f64>()
    {
        b.codebase_semantic.codebase_semantic_hybrid_alpha = Some(a);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_FTS_TOP_N")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_fts_top_n = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_HYBRID_SEMANTIC_POOL")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_hybrid_semantic_pool = Some(n);
    }
}
