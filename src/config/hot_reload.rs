//! 热重载时把新配置的可变子集合并进运行中的 [`super::types::AgentConfig`]。

use super::types::AgentConfig;
/// 将 **`load_config` 新结果** 中的「可热更」字段写入 `dst`，保留 **`dst` 中需进程级冻结的项**。
///
/// ## 边界（REPL **`/config reload`** / Web **`POST /config/reload`**）
///
/// - **`API_KEY`**：仍来自**进程环境**；本函数**不**读取或改写密钥，与启动时一致。
/// - **`conversation_store_sqlite_path`**：**不**热更（会话 SQLite 连接在启动时打开；改路径须重启 `serve`）。
/// - **`api_base` / `model` / `llm_http_auth_mode`**：从磁盘+环境变量**重新应用**（与 [`load_config`] 一致），**下一轮** LLM 请求起生效；共享 `reqwest::Client` 的连接池可能短暂保留旧主机空闲连接，直至池超时。
/// - **`health_llm_models_probe` / `health_llm_models_probe_cache_secs`**：热更后下一 **`GET /health`** 起生效；**不**自动清空进程内探测缓存（仍在 TTL 内会继续沿用旧结果直至过期）。
/// - **`system_prompt`**（含 **`system_prompt_file`** 重读）：从 `src` 写入，下一轮起生效。
/// - **`agent_tool_stats_*`**：热更后影响**下一轮起**附加段内容；已打开会话的 `system` 不会自动改写。
/// - **MCP**：`mcp_enabled` / `mcp_command` / `mcp_tool_timeout_secs` 会更新；调用方应在提交前 [`crate::mcp::clear_mcp_process_cache`].
pub fn apply_hot_reload_config_subset(dst: &mut AgentConfig, src: &AgentConfig) {
    dst.api_base.clone_from(&src.api_base);
    dst.model.clone_from(&src.model);
    dst.llm_http_auth_mode = src.llm_http_auth_mode;

    dst.max_message_history = src.max_message_history;
    dst.tui_load_session_on_start = src.tui_load_session_on_start;
    dst.tui_session_max_messages = src.tui_session_max_messages;
    dst.repl_initial_workspace_messages_enabled = src.repl_initial_workspace_messages_enabled;
    dst.command_timeout_secs = src.command_timeout_secs;
    dst.command_max_output_len = src.command_max_output_len;
    dst.allowed_commands = std::sync::Arc::clone(&src.allowed_commands);
    dst.run_command_working_dir
        .clone_from(&src.run_command_working_dir);
    dst.max_tokens = src.max_tokens;
    dst.temperature = src.temperature;
    dst.llm_seed = src.llm_seed;
    dst.llm_reasoning_split = src.llm_reasoning_split;
    dst.llm_bigmodel_thinking = src.llm_bigmodel_thinking;
    dst.llm_kimi_thinking_disabled = src.llm_kimi_thinking_disabled;
    dst.api_timeout_secs = src.api_timeout_secs;
    dst.api_max_retries = src.api_max_retries;
    dst.api_retry_delay_secs = src.api_retry_delay_secs;
    dst.weather_timeout_secs = src.weather_timeout_secs;
    dst.web_search_provider = src.web_search_provider;
    dst.web_search_api_key.clone_from(&src.web_search_api_key);
    dst.web_search_timeout_secs = src.web_search_timeout_secs;
    dst.web_search_max_results = src.web_search_max_results;
    dst.http_fetch_allowed_prefixes
        .clone_from(&src.http_fetch_allowed_prefixes);
    dst.http_fetch_timeout_secs = src.http_fetch_timeout_secs;
    dst.http_fetch_max_response_bytes = src.http_fetch_max_response_bytes;
    dst.reflection_default_max_rounds = src.reflection_default_max_rounds;
    dst.final_plan_requirement = src.final_plan_requirement;
    dst.plan_rewrite_max_attempts = src.plan_rewrite_max_attempts;
    dst.final_plan_require_strict_workflow_node_coverage =
        src.final_plan_require_strict_workflow_node_coverage;
    dst.final_plan_semantic_check_enabled = src.final_plan_semantic_check_enabled;
    dst.final_plan_semantic_check_max_non_readonly_tools =
        src.final_plan_semantic_check_max_non_readonly_tools;
    dst.final_plan_semantic_check_max_tokens = src.final_plan_semantic_check_max_tokens;
    dst.planner_executor_mode = src.planner_executor_mode;
    dst.system_prompt.clone_from(&src.system_prompt);
    dst.default_agent_role_id
        .clone_from(&src.default_agent_role_id);
    dst.agent_roles = std::sync::Arc::clone(&src.agent_roles);
    dst.cursor_rules_enabled = src.cursor_rules_enabled;
    dst.cursor_rules_dir.clone_from(&src.cursor_rules_dir);
    dst.cursor_rules_include_agents_md = src.cursor_rules_include_agents_md;
    dst.cursor_rules_max_chars = src.cursor_rules_max_chars;
    dst.tool_message_max_chars = src.tool_message_max_chars;
    dst.tool_result_envelope_v1 = src.tool_result_envelope_v1;
    dst.agent_tool_stats_enabled = src.agent_tool_stats_enabled;
    dst.agent_tool_stats_window_events = src.agent_tool_stats_window_events;
    dst.agent_tool_stats_min_samples = src.agent_tool_stats_min_samples;
    dst.agent_tool_stats_max_chars = src.agent_tool_stats_max_chars;
    dst.agent_tool_stats_warn_below_success_ratio = src.agent_tool_stats_warn_below_success_ratio;
    dst.materialize_deepseek_dsml_tool_calls = src.materialize_deepseek_dsml_tool_calls;
    dst.thinking_avoid_echo_system_prompt = src.thinking_avoid_echo_system_prompt;
    dst.thinking_avoid_echo_appendix
        .clone_from(&src.thinking_avoid_echo_appendix);
    dst.context_char_budget = src.context_char_budget;
    dst.context_min_messages_after_system = src.context_min_messages_after_system;
    dst.context_summary_trigger_chars = src.context_summary_trigger_chars;
    dst.context_summary_tail_messages = src.context_summary_tail_messages;
    dst.context_summary_max_tokens = src.context_summary_max_tokens;
    dst.context_summary_transcript_max_chars = src.context_summary_transcript_max_chars;
    dst.workspace_allowed_roots
        .clone_from(&src.workspace_allowed_roots);
    dst.web_api_bearer_token
        .clone_from(&src.web_api_bearer_token);
    dst.allow_insecure_no_auth_for_non_loopback = src.allow_insecure_no_auth_for_non_loopback;
    dst.health_llm_models_probe = src.health_llm_models_probe;
    dst.health_llm_models_probe_cache_secs = src.health_llm_models_probe_cache_secs;
    dst.chat_queue_max_concurrent = src.chat_queue_max_concurrent;
    dst.chat_queue_max_pending = src.chat_queue_max_pending;
    dst.parallel_readonly_tools_max = src.parallel_readonly_tools_max;
    dst.read_file_turn_cache_max_entries = src.read_file_turn_cache_max_entries;
    dst.test_result_cache_enabled = src.test_result_cache_enabled;
    dst.test_result_cache_max_entries = src.test_result_cache_max_entries;
    dst.session_workspace_changelist_enabled = src.session_workspace_changelist_enabled;
    dst.session_workspace_changelist_max_chars = src.session_workspace_changelist_max_chars;
    dst.staged_plan_execution = src.staged_plan_execution;
    dst.staged_plan_phase_instruction
        .clone_from(&src.staged_plan_phase_instruction);
    dst.staged_plan_allow_no_task = src.staged_plan_allow_no_task;
    dst.staged_plan_feedback_mode = src.staged_plan_feedback_mode;
    dst.staged_plan_patch_max_attempts = src.staged_plan_patch_max_attempts;
    dst.staged_plan_cli_show_planner_stream = src.staged_plan_cli_show_planner_stream;
    dst.staged_plan_optimizer_round = src.staged_plan_optimizer_round;
    dst.staged_plan_optimizer_requires_parallel_tools =
        src.staged_plan_optimizer_requires_parallel_tools;
    dst.staged_plan_ensemble_count = src.staged_plan_ensemble_count;
    dst.staged_plan_skip_ensemble_on_casual_prompt = src.staged_plan_skip_ensemble_on_casual_prompt;
    dst.staged_plan_two_phase_nl_display = src.staged_plan_two_phase_nl_display;
    dst.sync_default_tool_sandbox_mode = src.sync_default_tool_sandbox_mode;
    dst.sync_default_tool_sandbox_docker_image
        .clone_from(&src.sync_default_tool_sandbox_docker_image);
    dst.sync_default_tool_sandbox_docker_network
        .clone_from(&src.sync_default_tool_sandbox_docker_network);
    dst.sync_default_tool_sandbox_docker_timeout_secs =
        src.sync_default_tool_sandbox_docker_timeout_secs;
    dst.sync_default_tool_sandbox_docker_user = src.sync_default_tool_sandbox_docker_user.clone();
    dst.agent_memory_file_enabled = src.agent_memory_file_enabled;
    dst.agent_memory_file.clone_from(&src.agent_memory_file);
    dst.agent_memory_file_max_chars = src.agent_memory_file_max_chars;
    dst.living_docs_inject_enabled = src.living_docs_inject_enabled;
    dst.living_docs_relative_dir
        .clone_from(&src.living_docs_relative_dir);
    dst.living_docs_inject_max_chars = src.living_docs_inject_max_chars;
    dst.living_docs_file_max_each_chars = src.living_docs_file_max_each_chars;
    dst.project_profile_inject_enabled = src.project_profile_inject_enabled;
    dst.project_profile_inject_max_chars = src.project_profile_inject_max_chars;
    dst.project_dependency_brief_inject_enabled = src.project_dependency_brief_inject_enabled;
    dst.project_dependency_brief_inject_max_chars = src.project_dependency_brief_inject_max_chars;
    dst.tool_call_explain_enabled = src.tool_call_explain_enabled;
    dst.tool_call_explain_min_chars = src.tool_call_explain_min_chars;
    dst.tool_call_explain_max_chars = src.tool_call_explain_max_chars;
    dst.long_term_memory_enabled = src.long_term_memory_enabled;
    dst.long_term_memory_scope_mode = src.long_term_memory_scope_mode;
    dst.long_term_memory_vector_backend = src.long_term_memory_vector_backend;
    dst.long_term_memory_max_entries = src.long_term_memory_max_entries;
    dst.long_term_memory_inject_max_chars = src.long_term_memory_inject_max_chars;
    dst.long_term_memory_store_sqlite_path
        .clone_from(&src.long_term_memory_store_sqlite_path);
    dst.long_term_memory_top_k = src.long_term_memory_top_k;
    dst.long_term_memory_max_chars_per_chunk = src.long_term_memory_max_chars_per_chunk;
    dst.long_term_memory_min_chars_to_index = src.long_term_memory_min_chars_to_index;
    dst.long_term_memory_async_index = src.long_term_memory_async_index;
    dst.long_term_memory_auto_index_turns = src.long_term_memory_auto_index_turns;
    dst.long_term_memory_default_ttl_secs = src.long_term_memory_default_ttl_secs;
    dst.mcp_enabled = src.mcp_enabled;
    dst.mcp_command.clone_from(&src.mcp_command);
    dst.mcp_tool_timeout_secs = src.mcp_tool_timeout_secs;
    dst.codebase_semantic_search_enabled = src.codebase_semantic_search_enabled;
    dst.codebase_semantic_invalidate_on_workspace_change =
        src.codebase_semantic_invalidate_on_workspace_change;
    dst.codebase_semantic_index_sqlite_path
        .clone_from(&src.codebase_semantic_index_sqlite_path);
    dst.codebase_semantic_max_file_bytes = src.codebase_semantic_max_file_bytes;
    dst.codebase_semantic_chunk_max_chars = src.codebase_semantic_chunk_max_chars;
    dst.codebase_semantic_top_k = src.codebase_semantic_top_k;
    dst.codebase_semantic_query_max_chunks = src.codebase_semantic_query_max_chunks;
    dst.codebase_semantic_rebuild_max_files = src.codebase_semantic_rebuild_max_files;
    dst.codebase_semantic_rebuild_incremental = src.codebase_semantic_rebuild_incremental;
    dst.tool_registry_http_fetch_wall_timeout_secs = src.tool_registry_http_fetch_wall_timeout_secs;
    dst.tool_registry_http_request_wall_timeout_secs =
        src.tool_registry_http_request_wall_timeout_secs;
    dst.tool_registry_parallel_wall_timeout_secs =
        std::sync::Arc::clone(&src.tool_registry_parallel_wall_timeout_secs);
    dst.tool_registry_parallel_sync_denied_tools =
        src.tool_registry_parallel_sync_denied_tools.clone();
    dst.tool_registry_parallel_sync_denied_prefixes =
        src.tool_registry_parallel_sync_denied_prefixes.clone();
    dst.tool_registry_sync_default_inline_tools =
        src.tool_registry_sync_default_inline_tools.clone();
    dst.tool_registry_write_effect_tools = src.tool_registry_write_effect_tools.clone();
    dst.tool_registry_sub_agent_patch_write_extra_tools =
        src.tool_registry_sub_agent_patch_write_extra_tools.clone();
    dst.tool_registry_sub_agent_test_runner_extra_tools =
        src.tool_registry_sub_agent_test_runner_extra_tools.clone();
    dst.tool_registry_sub_agent_review_readonly_deny_tools = src
        .tool_registry_sub_agent_review_readonly_deny_tools
        .clone();
}
