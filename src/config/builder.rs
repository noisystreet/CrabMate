//! `ConfigBuilder`：嵌入 TOML 分片与用户 `[agent]` / `[tool_registry]` 的合并累加器。
//!
//! 由 [`super::assembly`] 与 [`super::env_overrides`] 写入字段，[`super::finalize`] 消费并产出 [`super::types::AgentConfig`]。

use std::collections::HashMap;

use super::agent_roles;
use super::source::{AgentRoleRow, AgentSection, ToolRegistrySection};

/// 配置累加器：依次接受嵌入默认 TOML → 用户配置文件 → 环境变量的覆盖，最终 `finalize` 为 `AgentConfig`。
#[derive(Default)]
pub(crate) struct ConfigBuilder {
    pub(crate) api_base: String,
    pub(crate) model: String,
    pub(crate) planner_model: Option<String>,
    pub(crate) executor_model: Option<String>,
    pub(crate) llm_http_auth_mode_str: Option<String>,
    pub(crate) system_prompt: String,
    pub(crate) system_prompt_file: Option<String>,
    pub(crate) max_message_history: Option<u64>,
    pub(crate) tui_load_session_on_start: Option<bool>,
    pub(crate) tui_session_max_messages: Option<u64>,
    pub(crate) repl_initial_workspace_messages_enabled: Option<bool>,
    pub(crate) command_timeout_secs: Option<u64>,
    pub(crate) command_max_output_len: Option<u64>,
    pub(crate) allowed_commands: Option<Vec<String>>,
    pub(crate) run_command_working_dir: Option<String>,
    pub(crate) max_tokens: Option<u64>,
    pub(crate) temperature: Option<f64>,
    pub(crate) llm_seed: Option<i64>,
    pub(crate) llm_reasoning_split: Option<bool>,
    pub(crate) llm_bigmodel_thinking: Option<bool>,
    pub(crate) llm_kimi_thinking_disabled: Option<bool>,
    pub(crate) api_timeout_secs: Option<u64>,
    pub(crate) api_max_retries: Option<u64>,
    pub(crate) api_retry_delay_secs: Option<u64>,
    pub(crate) weather_timeout_secs: Option<u64>,
    pub(crate) web_search_provider_str: Option<String>,
    pub(crate) web_search_api_key: Option<String>,
    pub(crate) web_search_timeout_secs: Option<u64>,
    pub(crate) web_search_max_results: Option<u64>,
    pub(crate) http_fetch_allowed_prefixes: Option<Vec<String>>,
    pub(crate) http_fetch_timeout_secs: Option<u64>,
    pub(crate) http_fetch_max_response_bytes: Option<u64>,
    pub(crate) reflection_default_max_rounds: Option<u64>,
    pub(crate) final_plan_requirement_str: Option<String>,
    pub(crate) plan_rewrite_max_attempts: Option<u64>,
    pub(crate) final_plan_require_strict_workflow_node_coverage: Option<bool>,
    pub(crate) final_plan_semantic_check_enabled: Option<bool>,
    pub(crate) final_plan_semantic_check_max_non_readonly_tools: Option<u64>,
    pub(crate) final_plan_semantic_check_max_tokens: Option<u64>,
    pub(crate) planner_executor_mode_str: Option<String>,
    pub(crate) cursor_rules_enabled: Option<bool>,
    pub(crate) cursor_rules_dir: Option<String>,
    pub(crate) cursor_rules_include_agents_md: Option<bool>,
    pub(crate) cursor_rules_max_chars: Option<u64>,
    pub(crate) skills_enabled: Option<bool>,
    pub(crate) skills_dir: Option<String>,
    pub(crate) skills_max_chars: Option<u64>,
    pub(crate) skills_top_k: Option<u64>,
    pub(crate) tool_message_max_chars: Option<u64>,
    pub(crate) tool_result_envelope_v1: Option<bool>,
    pub(crate) sse_tool_call_include_arguments: Option<bool>,
    /// 仅环境变量 `AGENT_THINKING_TRACE_ENABLED` 写入；**不**从 `[agent]` TOML 合并。
    pub(crate) agent_thinking_trace_enabled: Option<bool>,
    pub(crate) agent_tool_stats_enabled: Option<bool>,
    pub(crate) agent_tool_stats_window_events: Option<u64>,
    pub(crate) agent_tool_stats_min_samples: Option<u64>,
    pub(crate) agent_tool_stats_max_chars: Option<u64>,
    pub(crate) agent_tool_stats_warn_below_success_ratio: Option<f64>,
    pub(crate) materialize_deepseek_dsml_tool_calls: Option<bool>,
    pub(crate) thinking_avoid_echo_system_prompt: Option<bool>,
    pub(crate) thinking_avoid_echo_appendix: Option<String>,
    pub(crate) thinking_avoid_echo_appendix_file: Option<String>,
    pub(crate) context_char_budget: Option<u64>,
    pub(crate) context_min_messages_after_system: Option<u64>,
    pub(crate) context_summary_trigger_chars: Option<u64>,
    pub(crate) context_summary_tail_messages: Option<u64>,
    pub(crate) context_summary_max_tokens: Option<u64>,
    pub(crate) context_summary_transcript_max_chars: Option<u64>,
    pub(crate) health_llm_models_probe: Option<bool>,
    pub(crate) health_llm_models_probe_cache_secs: Option<u64>,
    pub(crate) chat_queue_max_concurrent: Option<u64>,
    pub(crate) chat_queue_max_pending: Option<u64>,
    pub(crate) parallel_readonly_tools_max: Option<u64>,
    pub(crate) read_file_turn_cache_max_entries: Option<u64>,
    pub(crate) test_result_cache_enabled: Option<bool>,
    pub(crate) test_result_cache_max_entries: Option<u64>,
    pub(crate) session_workspace_changelist_enabled: Option<bool>,
    pub(crate) session_workspace_changelist_max_chars: Option<u64>,
    pub(crate) staged_plan_execution: Option<bool>,
    pub(crate) staged_plan_phase_instruction: Option<String>,
    pub(crate) staged_plan_allow_no_task: Option<bool>,
    pub(crate) staged_plan_feedback_mode_str: Option<String>,
    pub(crate) staged_plan_patch_max_attempts: Option<u64>,
    pub(crate) staged_plan_cli_show_planner_stream: Option<bool>,
    pub(crate) staged_plan_optimizer_round: Option<bool>,
    pub(crate) staged_plan_optimizer_requires_parallel_tools: Option<bool>,
    pub(crate) staged_plan_ensemble_count: Option<u64>,
    pub(crate) staged_plan_skip_ensemble_on_casual_prompt: Option<bool>,
    pub(crate) staged_plan_two_phase_nl_display: Option<bool>,
    pub(crate) sync_default_tool_sandbox_mode_str: Option<String>,
    pub(crate) sync_default_tool_sandbox_docker_image: Option<String>,
    pub(crate) sync_default_tool_sandbox_docker_network: Option<String>,
    pub(crate) sync_default_tool_sandbox_docker_timeout_secs: Option<u64>,
    pub(crate) sync_default_tool_sandbox_docker_user: Option<String>,
    pub(crate) workspace_allowed_roots: Option<Vec<String>>,
    pub(crate) web_api_bearer_token: Option<String>,
    pub(crate) web_api_require_bearer: Option<bool>,
    pub(crate) allow_insecure_no_auth_for_non_loopback: Option<bool>,
    pub(crate) conversation_store_sqlite_path: Option<String>,
    pub(crate) agent_memory_file_enabled: Option<bool>,
    pub(crate) agent_memory_file: Option<String>,
    pub(crate) agent_memory_file_max_chars: Option<u64>,
    pub(crate) living_docs_inject_enabled: Option<bool>,
    pub(crate) living_docs_relative_dir: Option<String>,
    pub(crate) living_docs_inject_max_chars: Option<u64>,
    pub(crate) living_docs_file_max_each_chars: Option<u64>,
    pub(crate) project_profile_inject_enabled: Option<bool>,
    pub(crate) project_profile_inject_max_chars: Option<u64>,
    pub(crate) project_dependency_brief_inject_enabled: Option<bool>,
    pub(crate) project_dependency_brief_inject_max_chars: Option<u64>,
    pub(crate) tool_call_explain_enabled: Option<bool>,
    pub(crate) tool_call_explain_min_chars: Option<u64>,
    pub(crate) tool_call_explain_max_chars: Option<u64>,
    pub(crate) long_term_memory_enabled: Option<bool>,
    pub(crate) long_term_memory_scope_mode_str: Option<String>,
    pub(crate) long_term_memory_vector_backend_str: Option<String>,
    pub(crate) long_term_memory_max_entries: Option<u64>,
    pub(crate) long_term_memory_inject_max_chars: Option<u64>,
    pub(crate) long_term_memory_store_sqlite_path: Option<String>,
    pub(crate) long_term_memory_top_k: Option<u64>,
    pub(crate) long_term_memory_max_chars_per_chunk: Option<u64>,
    pub(crate) long_term_memory_min_chars_to_index: Option<u64>,
    pub(crate) long_term_memory_async_index: Option<bool>,
    pub(crate) long_term_memory_auto_index_turns: Option<bool>,
    pub(crate) long_term_memory_default_ttl_secs: Option<u64>,
    pub(crate) mcp_enabled: Option<bool>,
    pub(crate) mcp_command: Option<String>,
    pub(crate) mcp_tool_timeout_secs: Option<u64>,
    pub(crate) codebase_semantic_search_enabled: Option<bool>,
    pub(crate) codebase_semantic_invalidate_on_workspace_change: Option<bool>,
    pub(crate) codebase_semantic_index_sqlite_path: Option<String>,
    pub(crate) codebase_semantic_max_file_bytes: Option<u64>,
    pub(crate) codebase_semantic_chunk_max_chars: Option<u64>,
    pub(crate) codebase_semantic_top_k: Option<u64>,
    pub(crate) codebase_semantic_query_max_chunks: Option<u64>,
    pub(crate) codebase_semantic_rebuild_max_files: Option<u64>,
    pub(crate) codebase_semantic_rebuild_incremental: Option<bool>,
    pub(crate) codebase_semantic_hybrid_alpha: Option<f64>,
    pub(crate) codebase_semantic_fts_top_n: Option<u64>,
    pub(crate) codebase_semantic_hybrid_semantic_pool: Option<u64>,
    pub(crate) intent_execute_low_threshold: Option<f64>,
    pub(crate) intent_execute_high_threshold: Option<f64>,
    pub(crate) intent_mode_bias_enabled: Option<bool>,
    pub(crate) intent_l2_enabled: Option<bool>,
    pub(crate) intent_l2_min_confidence: Option<f64>,
    pub(crate) intent_l2_max_tokens: Option<u64>,
    pub(crate) intent_at_turn_start_enabled: Option<bool>,
    pub(crate) intent_l0_routing_boost_enabled: Option<bool>,
    /// 见 `[tool_registry]`：`http_fetch` spawn 外圈超时秒数
    pub(crate) tool_registry_http_fetch_wall_timeout_secs: Option<u64>,
    pub(crate) tool_registry_http_request_wall_timeout_secs: Option<u64>,
    pub(crate) tool_registry_parallel_wall_timeout_secs: HashMap<String, u64>,
    pub(crate) tool_registry_parallel_sync_denied_tools: Option<Vec<String>>,
    pub(crate) tool_registry_parallel_sync_denied_prefixes: Option<Vec<String>>,
    pub(crate) tool_registry_sync_default_inline_tools: Option<Vec<String>>,
    pub(crate) tool_registry_write_effect_tools: Option<Vec<String>>,
    pub(crate) tool_registry_sub_agent_patch_write_extra_tools: Option<Vec<String>>,
    pub(crate) tool_registry_sub_agent_test_runner_extra_tools: Option<Vec<String>>,
    pub(crate) tool_registry_sub_agent_review_readonly_deny_tools: Option<Vec<String>>,
    /// Web/CLI 未指定 `agent_role` 时使用的默认角色 id（须存在于角色表；与 `agent_roles.toml` / `AGENT_DEFAULT_AGENT_ROLE` 一致）
    pub(crate) default_agent_role_id: Option<String>,
    /// `id -> 未合并条目`；在 [`finalize`] 中与全局 cursor rules 设置一并落成 `AgentConfig.agent_roles`。
    pub(crate) agent_role_entries: HashMap<String, agent_roles::AgentRoleEntryBuilder>,
}

/// 非空 trim 后覆盖 `String` 字段。
pub(super) fn override_string(dst: &mut String, src: Option<String>) {
    if let Some(s) = src {
        let s = s.trim().to_string();
        if !s.is_empty() {
            *dst = s;
        }
    }
}

/// 非空 trim 后覆盖 `Option<String>` 字段。
pub(super) fn override_opt_string_non_empty(dst: &mut Option<String>, src: Option<String>) {
    if let Some(s) = src {
        let s = s.trim().to_string();
        if !s.is_empty() {
            *dst = Some(s);
        }
    }
}

/// trim 后覆盖 `Option<String>`（允许空字符串，如 bearer token 可显式清空）。
pub(super) fn override_opt_string_trimmed(dst: &mut Option<String>, src: Option<&String>) {
    if let Some(s) = src {
        *dst = Some(s.trim().to_string());
    }
}

/// 非空时覆盖 `Option<Vec<String>>`。
pub(super) fn override_opt_vec(dst: &mut Option<Vec<String>>, src: &Option<Vec<String>>) {
    if let Some(ref v) = *src
        && !v.is_empty()
    {
        *dst = Some(v.clone());
    }
}

impl ConfigBuilder {
    /// 将 `AgentSection` 中有值的字段覆盖到当前累加器。
    pub(super) fn apply_section(&mut self, agent: AgentSection) {
        override_string(&mut self.api_base, agent.api_base);
        override_string(&mut self.model, agent.model);
        override_opt_string_non_empty(&mut self.planner_model, agent.planner_model);
        override_opt_string_non_empty(&mut self.executor_model, agent.executor_model);
        override_opt_string_non_empty(&mut self.llm_http_auth_mode_str, agent.llm_http_auth_mode);
        let no_system_prompt_file_in_section = agent.system_prompt_file.is_none();
        let inline_system_prompt_nonempty = agent
            .system_prompt
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        override_opt_string_non_empty(&mut self.system_prompt_file, agent.system_prompt_file);
        override_string(&mut self.system_prompt, agent.system_prompt);
        override_opt_string_non_empty(&mut self.default_agent_role_id, agent.default_agent_role);
        // 本段若只给了内联 system_prompt、未再给 system_prompt_file，则不再沿用更早层（如嵌入默认）的文件路径
        if no_system_prompt_file_in_section && inline_system_prompt_nonempty {
            self.system_prompt_file = None;
        }
        override_opt_string_non_empty(
            &mut self.run_command_working_dir,
            agent.run_command_working_dir,
        );
        override_opt_string_non_empty(&mut self.web_search_provider_str, agent.web_search_provider);
        override_opt_string_non_empty(
            &mut self.final_plan_requirement_str,
            agent.final_plan_requirement,
        );
        override_opt_string_non_empty(
            &mut self.planner_executor_mode_str,
            agent.planner_executor_mode,
        );
        override_opt_string_non_empty(&mut self.cursor_rules_dir, agent.cursor_rules_dir);
        override_opt_string_non_empty(&mut self.skills_dir, agent.skills_dir);

        override_opt_string_trimmed(
            &mut self.web_api_bearer_token,
            agent.web_api_bearer_token.as_ref(),
        );
        override_opt_string_trimmed(
            &mut self.staged_plan_phase_instruction,
            agent.staged_plan_phase_instruction.as_ref(),
        );
        if let Some(ref k) = agent.web_search_api_key {
            self.web_search_api_key = Some(k.clone());
        }

        override_opt_vec(&mut self.allowed_commands, &agent.allowed_commands);
        override_opt_vec(
            &mut self.http_fetch_allowed_prefixes,
            &agent.http_fetch_allowed_prefixes,
        );
        override_opt_vec(
            &mut self.workspace_allowed_roots,
            &agent.workspace_allowed_roots,
        );

        self.max_message_history = agent.max_message_history.or(self.max_message_history);
        self.tui_load_session_on_start = agent
            .tui_load_session_on_start
            .or(self.tui_load_session_on_start);
        self.tui_session_max_messages = agent
            .tui_session_max_messages
            .or(self.tui_session_max_messages);
        self.repl_initial_workspace_messages_enabled = agent
            .repl_initial_workspace_messages_enabled
            .or(self.repl_initial_workspace_messages_enabled);
        self.command_timeout_secs = agent.command_timeout_secs.or(self.command_timeout_secs);
        self.command_max_output_len = agent.command_max_output_len.or(self.command_max_output_len);
        self.max_tokens = agent.max_tokens.or(self.max_tokens);
        self.temperature = agent.temperature.or(self.temperature);
        self.llm_seed = agent.llm_seed.or(self.llm_seed);
        self.llm_reasoning_split = agent.llm_reasoning_split.or(self.llm_reasoning_split);
        self.llm_bigmodel_thinking = agent.llm_bigmodel_thinking.or(self.llm_bigmodel_thinking);
        self.llm_kimi_thinking_disabled = agent
            .llm_kimi_thinking_disabled
            .or(self.llm_kimi_thinking_disabled);
        self.api_timeout_secs = agent.api_timeout_secs.or(self.api_timeout_secs);
        self.api_max_retries = agent.api_max_retries.or(self.api_max_retries);
        self.api_retry_delay_secs = agent.api_retry_delay_secs.or(self.api_retry_delay_secs);
        self.weather_timeout_secs = agent.weather_timeout_secs.or(self.weather_timeout_secs);
        self.web_search_timeout_secs = agent
            .web_search_timeout_secs
            .or(self.web_search_timeout_secs);
        self.web_search_max_results = agent.web_search_max_results.or(self.web_search_max_results);
        self.http_fetch_timeout_secs = agent
            .http_fetch_timeout_secs
            .or(self.http_fetch_timeout_secs);
        self.http_fetch_max_response_bytes = agent
            .http_fetch_max_response_bytes
            .or(self.http_fetch_max_response_bytes);
        self.reflection_default_max_rounds = agent
            .reflection_default_max_rounds
            .or(self.reflection_default_max_rounds);
        self.plan_rewrite_max_attempts = agent
            .plan_rewrite_max_attempts
            .or(self.plan_rewrite_max_attempts);
        self.final_plan_require_strict_workflow_node_coverage = agent
            .final_plan_require_strict_workflow_node_coverage
            .or(self.final_plan_require_strict_workflow_node_coverage);
        self.final_plan_semantic_check_enabled = agent
            .final_plan_semantic_check_enabled
            .or(self.final_plan_semantic_check_enabled);
        self.final_plan_semantic_check_max_non_readonly_tools = agent
            .final_plan_semantic_check_max_non_readonly_tools
            .or(self.final_plan_semantic_check_max_non_readonly_tools);
        self.final_plan_semantic_check_max_tokens = agent
            .final_plan_semantic_check_max_tokens
            .or(self.final_plan_semantic_check_max_tokens);
        self.cursor_rules_enabled = agent.cursor_rules_enabled.or(self.cursor_rules_enabled);
        self.cursor_rules_include_agents_md = agent
            .cursor_rules_include_agents_md
            .or(self.cursor_rules_include_agents_md);
        self.cursor_rules_max_chars = agent.cursor_rules_max_chars.or(self.cursor_rules_max_chars);
        self.skills_enabled = agent.skills_enabled.or(self.skills_enabled);
        self.skills_max_chars = agent.skills_max_chars.or(self.skills_max_chars);
        self.skills_top_k = agent.skills_top_k.or(self.skills_top_k);
        self.tool_message_max_chars = agent.tool_message_max_chars.or(self.tool_message_max_chars);
        self.tool_result_envelope_v1 = agent
            .tool_result_envelope_v1
            .or(self.tool_result_envelope_v1);
        self.sse_tool_call_include_arguments = agent
            .sse_tool_call_include_arguments
            .or(self.sse_tool_call_include_arguments);
        self.agent_tool_stats_enabled = agent
            .agent_tool_stats_enabled
            .or(self.agent_tool_stats_enabled);
        self.agent_tool_stats_window_events = agent
            .agent_tool_stats_window_events
            .or(self.agent_tool_stats_window_events);
        self.agent_tool_stats_min_samples = agent
            .agent_tool_stats_min_samples
            .or(self.agent_tool_stats_min_samples);
        self.agent_tool_stats_max_chars = agent
            .agent_tool_stats_max_chars
            .or(self.agent_tool_stats_max_chars);
        self.agent_tool_stats_warn_below_success_ratio = agent
            .agent_tool_stats_warn_below_success_ratio
            .or(self.agent_tool_stats_warn_below_success_ratio);
        self.materialize_deepseek_dsml_tool_calls = agent
            .materialize_deepseek_dsml_tool_calls
            .or(self.materialize_deepseek_dsml_tool_calls);
        self.thinking_avoid_echo_system_prompt = agent
            .thinking_avoid_echo_system_prompt
            .or(self.thinking_avoid_echo_system_prompt);
        let no_thinking_appendix_file_in_section =
            agent.thinking_avoid_echo_appendix_file.is_none();
        let inline_thinking_appendix_nonempty = agent
            .thinking_avoid_echo_appendix
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        override_opt_string_non_empty(
            &mut self.thinking_avoid_echo_appendix_file,
            agent.thinking_avoid_echo_appendix_file.clone(),
        );
        if let Some(ref s) = agent.thinking_avoid_echo_appendix
            && !s.trim().is_empty()
        {
            self.thinking_avoid_echo_appendix = Some(s.clone());
        }
        if no_thinking_appendix_file_in_section && inline_thinking_appendix_nonempty {
            self.thinking_avoid_echo_appendix_file = None;
        }
        self.context_char_budget = agent.context_char_budget.or(self.context_char_budget);
        self.context_min_messages_after_system = agent
            .context_min_messages_after_system
            .or(self.context_min_messages_after_system);
        self.context_summary_trigger_chars = agent
            .context_summary_trigger_chars
            .or(self.context_summary_trigger_chars);
        self.context_summary_tail_messages = agent
            .context_summary_tail_messages
            .or(self.context_summary_tail_messages);
        self.context_summary_max_tokens = agent
            .context_summary_max_tokens
            .or(self.context_summary_max_tokens);
        self.context_summary_transcript_max_chars = agent
            .context_summary_transcript_max_chars
            .or(self.context_summary_transcript_max_chars);
        self.health_llm_models_probe = agent
            .health_llm_models_probe
            .or(self.health_llm_models_probe);
        self.health_llm_models_probe_cache_secs = agent
            .health_llm_models_probe_cache_secs
            .or(self.health_llm_models_probe_cache_secs);
        self.chat_queue_max_concurrent = agent
            .chat_queue_max_concurrent
            .or(self.chat_queue_max_concurrent);
        self.chat_queue_max_pending = agent.chat_queue_max_pending.or(self.chat_queue_max_pending);
        self.parallel_readonly_tools_max = agent
            .parallel_readonly_tools_max
            .or(self.parallel_readonly_tools_max);
        self.read_file_turn_cache_max_entries = agent
            .read_file_turn_cache_max_entries
            .or(self.read_file_turn_cache_max_entries);
        self.test_result_cache_enabled = agent
            .test_result_cache_enabled
            .or(self.test_result_cache_enabled);
        self.test_result_cache_max_entries = agent
            .test_result_cache_max_entries
            .or(self.test_result_cache_max_entries);
        self.session_workspace_changelist_enabled = agent
            .session_workspace_changelist_enabled
            .or(self.session_workspace_changelist_enabled);
        self.session_workspace_changelist_max_chars = agent
            .session_workspace_changelist_max_chars
            .or(self.session_workspace_changelist_max_chars);
        self.staged_plan_execution = agent.staged_plan_execution.or(self.staged_plan_execution);
        self.staged_plan_allow_no_task = agent
            .staged_plan_allow_no_task
            .or(self.staged_plan_allow_no_task);
        override_opt_string_non_empty(
            &mut self.staged_plan_feedback_mode_str,
            agent.staged_plan_feedback_mode,
        );
        self.staged_plan_patch_max_attempts = agent
            .staged_plan_patch_max_attempts
            .or(self.staged_plan_patch_max_attempts);
        self.staged_plan_cli_show_planner_stream = agent
            .staged_plan_cli_show_planner_stream
            .or(self.staged_plan_cli_show_planner_stream);
        self.staged_plan_optimizer_round = agent
            .staged_plan_optimizer_round
            .or(self.staged_plan_optimizer_round);
        self.staged_plan_optimizer_requires_parallel_tools = agent
            .staged_plan_optimizer_requires_parallel_tools
            .or(self.staged_plan_optimizer_requires_parallel_tools);
        self.staged_plan_ensemble_count = agent
            .staged_plan_ensemble_count
            .or(self.staged_plan_ensemble_count);
        self.staged_plan_skip_ensemble_on_casual_prompt = agent
            .staged_plan_skip_ensemble_on_casual_prompt
            .or(self.staged_plan_skip_ensemble_on_casual_prompt);
        self.staged_plan_two_phase_nl_display = agent
            .staged_plan_two_phase_nl_display
            .or(self.staged_plan_two_phase_nl_display);
        override_opt_string_non_empty(
            &mut self.sync_default_tool_sandbox_mode_str,
            agent.sync_default_tool_sandbox_mode,
        );
        override_opt_string_non_empty(
            &mut self.sync_default_tool_sandbox_docker_image,
            agent.sync_default_tool_sandbox_docker_image,
        );
        override_opt_string_non_empty(
            &mut self.sync_default_tool_sandbox_docker_network,
            agent.sync_default_tool_sandbox_docker_network,
        );
        self.sync_default_tool_sandbox_docker_timeout_secs = agent
            .sync_default_tool_sandbox_docker_timeout_secs
            .or(self.sync_default_tool_sandbox_docker_timeout_secs);
        override_opt_string_non_empty(
            &mut self.sync_default_tool_sandbox_docker_user,
            agent.sync_default_tool_sandbox_docker_user,
        );
        self.web_api_require_bearer = agent.web_api_require_bearer.or(self.web_api_require_bearer);
        self.allow_insecure_no_auth_for_non_loopback = agent
            .allow_insecure_no_auth_for_non_loopback
            .or(self.allow_insecure_no_auth_for_non_loopback);
        override_opt_string_non_empty(
            &mut self.conversation_store_sqlite_path,
            agent.conversation_store_sqlite_path,
        );
        self.agent_memory_file_enabled = agent
            .agent_memory_file_enabled
            .or(self.agent_memory_file_enabled);
        override_opt_string_non_empty(&mut self.agent_memory_file, agent.agent_memory_file);
        self.agent_memory_file_max_chars = agent
            .agent_memory_file_max_chars
            .or(self.agent_memory_file_max_chars);
        self.living_docs_inject_enabled = agent
            .living_docs_inject_enabled
            .or(self.living_docs_inject_enabled);
        override_opt_string_non_empty(
            &mut self.living_docs_relative_dir,
            agent.living_docs_relative_dir,
        );
        self.living_docs_inject_max_chars = agent
            .living_docs_inject_max_chars
            .or(self.living_docs_inject_max_chars);
        self.living_docs_file_max_each_chars = agent
            .living_docs_file_max_each_chars
            .or(self.living_docs_file_max_each_chars);
        self.project_profile_inject_enabled = agent
            .project_profile_inject_enabled
            .or(self.project_profile_inject_enabled);
        self.project_profile_inject_max_chars = agent
            .project_profile_inject_max_chars
            .or(self.project_profile_inject_max_chars);
        self.project_dependency_brief_inject_enabled = agent
            .project_dependency_brief_inject_enabled
            .or(self.project_dependency_brief_inject_enabled);
        self.project_dependency_brief_inject_max_chars = agent
            .project_dependency_brief_inject_max_chars
            .or(self.project_dependency_brief_inject_max_chars);
        self.tool_call_explain_enabled = agent
            .tool_call_explain_enabled
            .or(self.tool_call_explain_enabled);
        self.tool_call_explain_min_chars = agent
            .tool_call_explain_min_chars
            .or(self.tool_call_explain_min_chars);
        self.tool_call_explain_max_chars = agent
            .tool_call_explain_max_chars
            .or(self.tool_call_explain_max_chars);
        self.long_term_memory_enabled = agent
            .long_term_memory_enabled
            .or(self.long_term_memory_enabled);
        override_opt_string_non_empty(
            &mut self.long_term_memory_scope_mode_str,
            agent.long_term_memory_scope_mode,
        );
        override_opt_string_non_empty(
            &mut self.long_term_memory_vector_backend_str,
            agent.long_term_memory_vector_backend,
        );
        self.long_term_memory_max_entries = agent
            .long_term_memory_max_entries
            .or(self.long_term_memory_max_entries);
        self.long_term_memory_inject_max_chars = agent
            .long_term_memory_inject_max_chars
            .or(self.long_term_memory_inject_max_chars);
        override_opt_string_non_empty(
            &mut self.long_term_memory_store_sqlite_path,
            agent.long_term_memory_store_sqlite_path,
        );
        self.long_term_memory_top_k = agent.long_term_memory_top_k.or(self.long_term_memory_top_k);
        self.long_term_memory_max_chars_per_chunk = agent
            .long_term_memory_max_chars_per_chunk
            .or(self.long_term_memory_max_chars_per_chunk);
        self.long_term_memory_min_chars_to_index = agent
            .long_term_memory_min_chars_to_index
            .or(self.long_term_memory_min_chars_to_index);
        self.long_term_memory_async_index = agent
            .long_term_memory_async_index
            .or(self.long_term_memory_async_index);
        self.long_term_memory_auto_index_turns = agent
            .long_term_memory_auto_index_turns
            .or(self.long_term_memory_auto_index_turns);
        self.long_term_memory_default_ttl_secs = agent
            .long_term_memory_default_ttl_secs
            .or(self.long_term_memory_default_ttl_secs);
        self.mcp_enabled = agent.mcp_enabled.or(self.mcp_enabled);
        override_opt_string_non_empty(&mut self.mcp_command, agent.mcp_command);
        self.mcp_tool_timeout_secs = agent.mcp_tool_timeout_secs.or(self.mcp_tool_timeout_secs);
        self.codebase_semantic_search_enabled = agent
            .codebase_semantic_search_enabled
            .or(self.codebase_semantic_search_enabled);
        self.codebase_semantic_invalidate_on_workspace_change = agent
            .codebase_semantic_invalidate_on_workspace_change
            .or(self.codebase_semantic_invalidate_on_workspace_change);
        override_opt_string_non_empty(
            &mut self.codebase_semantic_index_sqlite_path,
            agent.codebase_semantic_index_sqlite_path,
        );
        self.codebase_semantic_max_file_bytes = agent
            .codebase_semantic_max_file_bytes
            .or(self.codebase_semantic_max_file_bytes);
        self.codebase_semantic_chunk_max_chars = agent
            .codebase_semantic_chunk_max_chars
            .or(self.codebase_semantic_chunk_max_chars);
        self.codebase_semantic_top_k = agent
            .codebase_semantic_top_k
            .or(self.codebase_semantic_top_k);
        self.codebase_semantic_query_max_chunks = agent
            .codebase_semantic_query_max_chunks
            .or(self.codebase_semantic_query_max_chunks);
        self.codebase_semantic_rebuild_max_files = agent
            .codebase_semantic_rebuild_max_files
            .or(self.codebase_semantic_rebuild_max_files);
        self.codebase_semantic_rebuild_incremental = agent
            .codebase_semantic_rebuild_incremental
            .or(self.codebase_semantic_rebuild_incremental);
        self.codebase_semantic_hybrid_alpha = agent
            .codebase_semantic_hybrid_alpha
            .or(self.codebase_semantic_hybrid_alpha);
        self.codebase_semantic_fts_top_n = agent
            .codebase_semantic_fts_top_n
            .or(self.codebase_semantic_fts_top_n);
        self.codebase_semantic_hybrid_semantic_pool = agent
            .codebase_semantic_hybrid_semantic_pool
            .or(self.codebase_semantic_hybrid_semantic_pool);
        self.intent_execute_low_threshold = agent
            .intent_execute_low_threshold
            .or(self.intent_execute_low_threshold);
        self.intent_execute_high_threshold = agent
            .intent_execute_high_threshold
            .or(self.intent_execute_high_threshold);
        self.intent_mode_bias_enabled = agent
            .intent_mode_bias_enabled
            .or(self.intent_mode_bias_enabled);
        self.intent_l2_enabled = agent.intent_l2_enabled.or(self.intent_l2_enabled);
        self.intent_l2_min_confidence = agent
            .intent_l2_min_confidence
            .or(self.intent_l2_min_confidence);
        self.intent_l2_max_tokens = agent.intent_l2_max_tokens.or(self.intent_l2_max_tokens);
        self.intent_at_turn_start_enabled = agent
            .intent_at_turn_start_enabled
            .or(self.intent_at_turn_start_enabled);
        self.intent_l0_routing_boost_enabled = agent
            .intent_l0_routing_boost_enabled
            .or(self.intent_l0_routing_boost_enabled);
    }

    pub(super) fn merge_agent_role_rows(&mut self, rows: &[AgentRoleRow]) {
        for row in rows {
            let id = row.id.trim().to_string();
            if id.is_empty() {
                continue;
            }
            let slot = self.agent_role_entries.entry(id).or_default();
            if let Some(ref p) = row.system_prompt {
                let p = p.trim().to_string();
                if !p.is_empty() {
                    slot.system_prompt = Some(p);
                }
            }
            if let Some(ref f) = row.system_prompt_file {
                let f = f.trim().to_string();
                if !f.is_empty() {
                    slot.system_prompt_file = Some(f);
                }
            }
            if let Some(list) = row.allowed_tools.clone() {
                slot.allowed_tools = Some(list);
            }
        }
    }

    pub(super) fn apply_tool_registry(&mut self, tr: ToolRegistrySection) {
        if let Some(v) = tr.http_fetch_wall_timeout_secs {
            self.tool_registry_http_fetch_wall_timeout_secs = Some(v);
        }
        if let Some(v) = tr.http_request_wall_timeout_secs {
            self.tool_registry_http_request_wall_timeout_secs = Some(v);
        }
        for (k, v) in tr.parallel_wall_timeout_secs {
            self.tool_registry_parallel_wall_timeout_secs.insert(k, v);
        }
        if let Some(v) = tr.parallel_sync_denied_tools {
            self.tool_registry_parallel_sync_denied_tools = Some(v);
        }
        if let Some(v) = tr.parallel_sync_denied_prefixes {
            self.tool_registry_parallel_sync_denied_prefixes = Some(v);
        }
        if let Some(v) = tr.sync_default_inline_tools {
            self.tool_registry_sync_default_inline_tools = Some(v);
        }
        if let Some(v) = tr.write_effect_tools {
            self.tool_registry_write_effect_tools = Some(v);
        }
        if let Some(v) = tr.sub_agent_patch_write_extra_tools {
            self.tool_registry_sub_agent_patch_write_extra_tools = Some(v);
        }
        if let Some(v) = tr.sub_agent_test_runner_extra_tools {
            self.tool_registry_sub_agent_test_runner_extra_tools = Some(v);
        }
        if let Some(v) = tr.sub_agent_review_readonly_deny_tools {
            self.tool_registry_sub_agent_review_readonly_deny_tools = Some(v);
        }
    }
}
