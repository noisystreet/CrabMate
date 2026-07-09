//! `ConfigBuilder`：嵌入 TOML 分片与用户 `[agent]` / `[tool_registry]` 的合并累加器。
//!
//! 结构按运行域拆分为子结构（与 [`super::types::AgentConfig`] 对齐），见 **`config_builder_sections`**。
//! 由 [`super::assembly`] 与 [`super::env_overrides`] 写入字段，[`super::finalize`] 消费并产出 [`super::types::AgentConfig`]。

mod config_builder_sections;

pub(crate) use config_builder_sections::ConfigBuilder;

use super::source::{AgentRoleRow, AgentSection, ScheduledAgentTaskRow, ToolRegistrySection};

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
        self.apply_section_identity_prompt_and_lists(&agent);
        self.apply_section_merge_numeric_mid(&agent);
        self.apply_section_merge_numeric_tail_queues_staged(&agent);
        self.apply_section_merge_numeric_tail_sandbox_web_conv(&agent);
        self.apply_section_merge_numeric_tail_context_tool_explain(&agent);
        self.apply_section_merge_numeric_tail_memory_mcp_semantic_intent(&agent);
    }

    /// 标识字段、提示词路径、列表类覆盖。
    fn apply_section_identity_prompt_and_lists(&mut self, agent: &AgentSection) {
        let llm = &mut self.llm;
        override_string(&mut llm.api_base, agent.api_base.clone());
        override_string(&mut llm.model, agent.model.clone());
        override_opt_string_non_empty(&mut llm.planner_model, agent.planner_model.clone());
        override_opt_string_non_empty(&mut llm.executor_model, agent.executor_model.clone());
        override_opt_string_non_empty(
            &mut llm.llm_http_auth_mode_str,
            agent.llm_http_auth_mode.clone(),
        );
        let rp = &mut self.roles_prompts;
        let no_system_prompt_file_in_section = agent.system_prompt_file.is_none();
        let inline_system_prompt_nonempty = agent
            .system_prompt
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        override_opt_string_non_empty(&mut rp.system_prompt_file, agent.system_prompt_file.clone());
        override_string(&mut rp.system_prompt, agent.system_prompt.clone());
        override_opt_string_non_empty(
            &mut rp.default_agent_role_id,
            agent.default_agent_role.clone(),
        );
        rp.coding_workbench_enabled = agent
            .coding_workbench_enabled
            .or(rp.coding_workbench_enabled);
        override_opt_string_non_empty(
            &mut rp.coding_workbench_increment_file,
            agent.coding_workbench_increment_file.clone(),
        );
        if no_system_prompt_file_in_section && inline_system_prompt_nonempty {
            rp.system_prompt_file = None;
        }
        override_opt_string_non_empty(
            &mut self.command_exec.run_command_working_dir,
            agent.run_command_working_dir.clone(),
        );
        override_opt_string_non_empty(
            &mut self.web_search.web_search_provider_str,
            agent.web_search_provider.clone(),
        );
        override_opt_string_non_empty(
            &mut self.per_plan_policy.final_plan_requirement_str,
            agent.final_plan_requirement.clone(),
        );
        override_opt_string_non_empty(
            &mut self.per_plan_policy.planner_executor_mode_str,
            agent.planner_executor_mode.clone(),
        );
        override_opt_string_non_empty(
            &mut self.per_plan_policy.orchestration_profile_str,
            agent.orchestration_profile.clone(),
        );
        override_opt_string_non_empty(
            &mut self.per_plan_policy.orchestration_decision_mode_str,
            agent.orchestration_decision_mode.clone(),
        );
        let pp = &mut self.per_plan_policy;
        pp.decision_staged_threshold = agent
            .decision_staged_threshold
            .or(pp.decision_staged_threshold);
        pp.decision_weight_intent = agent.decision_weight_intent.or(pp.decision_weight_intent);
        pp.decision_weight_complexity = agent
            .decision_weight_complexity
            .or(pp.decision_weight_complexity);
        pp.decision_weight_workspace = agent
            .decision_weight_workspace
            .or(pp.decision_weight_workspace);
        pp.decision_weight_history = agent.decision_weight_history.or(pp.decision_weight_history);
        pp.decision_weight_cost = agent.decision_weight_cost.or(pp.decision_weight_cost);
        override_opt_string_non_empty(
            &mut self.cursor_rules.cursor_rules_dir,
            agent.cursor_rules_dir.clone(),
        );
        override_opt_string_non_empty(&mut self.skills.skills_dir, agent.skills_dir.clone());

        override_opt_string_trimmed(
            &mut self.web_api.web_api_bearer_token,
            agent.web_api_bearer_token.as_ref(),
        );
        override_opt_string_trimmed(
            &mut self.staged_planning.staged_plan_phase_instruction,
            agent.staged_plan_phase_instruction.as_ref(),
        );
        if let Some(ref k) = agent.web_search_api_key {
            self.web_search.web_search_api_key = Some(k.clone());
        }

        override_opt_vec(
            &mut self.command_exec.allowed_commands,
            &agent.allowed_commands,
        );
        override_opt_vec(
            &mut self.http_fetch.http_fetch_allowed_prefixes,
            &agent.http_fetch_allowed_prefixes,
        );
        override_opt_vec(
            &mut self.workspace_roots.workspace_allowed_roots,
            &agent.workspace_allowed_roots,
        );
    }

    /// `Option` 数值与布尔合并（至上下文摘要与健康探测）。
    fn apply_section_merge_numeric_mid(&mut self, agent: &AgentSection) {
        let su = &mut self.session_ui;
        su.max_message_history = agent.max_message_history.or(su.max_message_history);
        su.tui_load_session_on_start = agent
            .tui_load_session_on_start
            .or(su.tui_load_session_on_start);
        su.tui_session_max_messages = agent
            .tui_session_max_messages
            .or(su.tui_session_max_messages);
        su.repl_initial_workspace_messages_enabled = agent
            .repl_initial_workspace_messages_enabled
            .or(su.repl_initial_workspace_messages_enabled);
        let ce = &mut self.command_exec;
        ce.command_timeout_secs = agent.command_timeout_secs.or(ce.command_timeout_secs);
        ce.command_max_output_len = agent.command_max_output_len.or(ce.command_max_output_len);
        let samp = &mut self.llm_sampling;
        samp.max_tokens = agent.max_tokens.or(samp.max_tokens);
        samp.llm_context_tokens = agent.llm_context_tokens.or(samp.llm_context_tokens);
        samp.temperature = agent.temperature.or(samp.temperature);
        samp.llm_seed = agent.llm_seed.or(samp.llm_seed);
        let lv = &mut self.llm_vendor;
        lv.llm_reasoning_split = agent.llm_reasoning_split.or(lv.llm_reasoning_split);
        lv.llm_bigmodel_thinking = agent.llm_bigmodel_thinking.or(lv.llm_bigmodel_thinking);
        lv.llm_kimi_thinking_disabled = agent
            .llm_kimi_thinking_disabled
            .or(lv.llm_kimi_thinking_disabled);
        let retry = &mut self.llm_http_retry;
        retry.api_timeout_secs = agent.api_timeout_secs.or(retry.api_timeout_secs);
        retry.api_max_retries = agent.api_max_retries.or(retry.api_max_retries);
        retry.api_retry_delay_secs = agent.api_retry_delay_secs.or(retry.api_retry_delay_secs);
        self.weather_tool.weather_timeout_secs = agent
            .weather_timeout_secs
            .or(self.weather_tool.weather_timeout_secs);
        let ws = &mut self.web_search;
        ws.web_search_timeout_secs = agent.web_search_timeout_secs.or(ws.web_search_timeout_secs);
        ws.web_search_max_results = agent.web_search_max_results.or(ws.web_search_max_results);
        let hf = &mut self.http_fetch;
        hf.http_fetch_timeout_secs = agent.http_fetch_timeout_secs.or(hf.http_fetch_timeout_secs);
        hf.http_fetch_max_response_bytes = agent
            .http_fetch_max_response_bytes
            .or(hf.http_fetch_max_response_bytes);
        let pp = &mut self.per_plan_policy;
        pp.reflection_default_max_rounds = agent
            .reflection_default_max_rounds
            .or(pp.reflection_default_max_rounds);
        pp.plan_rewrite_max_attempts = agent
            .plan_rewrite_max_attempts
            .or(pp.plan_rewrite_max_attempts);
        pp.final_plan_require_strict_workflow_node_coverage = agent
            .final_plan_require_strict_workflow_node_coverage
            .or(pp.final_plan_require_strict_workflow_node_coverage);
        pp.final_plan_semantic_check_enabled = agent
            .final_plan_semantic_check_enabled
            .or(pp.final_plan_semantic_check_enabled);
        pp.final_plan_semantic_check_max_non_readonly_tools = agent
            .final_plan_semantic_check_max_non_readonly_tools
            .or(pp.final_plan_semantic_check_max_non_readonly_tools);
        pp.final_plan_semantic_check_max_tokens = agent
            .final_plan_semantic_check_max_tokens
            .or(pp.final_plan_semantic_check_max_tokens);
        let cr = &mut self.cursor_rules;
        cr.cursor_rules_enabled = agent.cursor_rules_enabled.or(cr.cursor_rules_enabled);
        cr.cursor_rules_include_agents_md = agent
            .cursor_rules_include_agents_md
            .or(cr.cursor_rules_include_agents_md);
        cr.cursor_rules_max_chars = agent.cursor_rules_max_chars.or(cr.cursor_rules_max_chars);
        let sk = &mut self.skills;
        sk.skills_enabled = agent.skills_enabled.or(sk.skills_enabled);
        sk.skills_max_chars = agent.skills_max_chars.or(sk.skills_max_chars);
        sk.skills_top_k = agent.skills_top_k.or(sk.skills_top_k);
        let tt = &mut self.tool_transcript;
        tt.tool_message_max_chars = agent.tool_message_max_chars.or(tt.tool_message_max_chars);
        tt.tool_result_envelope_v1 = agent.tool_result_envelope_v1.or(tt.tool_result_envelope_v1);
        tt.sse_tool_call_include_arguments = agent
            .sse_tool_call_include_arguments
            .or(tt.sse_tool_call_include_arguments);
        let ats = &mut self.agent_tool_stats;
        ats.agent_tool_stats_enabled = agent
            .agent_tool_stats_enabled
            .or(ats.agent_tool_stats_enabled);
        ats.agent_tool_stats_window_events = agent
            .agent_tool_stats_window_events
            .or(ats.agent_tool_stats_window_events);
        ats.agent_tool_stats_min_samples = agent
            .agent_tool_stats_min_samples
            .or(ats.agent_tool_stats_min_samples);
        ats.agent_tool_stats_max_chars = agent
            .agent_tool_stats_max_chars
            .or(ats.agent_tool_stats_max_chars);
        ats.agent_tool_stats_warn_below_success_ratio = agent
            .agent_tool_stats_warn_below_success_ratio
            .or(ats.agent_tool_stats_warn_below_success_ratio);
        self.dsml_materialize.materialize_deepseek_dsml_tool_calls = agent
            .materialize_deepseek_dsml_tool_calls
            .or(self.dsml_materialize.materialize_deepseek_dsml_tool_calls);
        self.dsml_materialize.dsml_stream_strip_enabled = agent
            .dsml_stream_strip_enabled
            .or(self.dsml_materialize.dsml_stream_strip_enabled);
        let te = &mut self.thinking_echo;
        te.thinking_avoid_echo_system_prompt = agent
            .thinking_avoid_echo_system_prompt
            .or(te.thinking_avoid_echo_system_prompt);
        let no_thinking_appendix_file_in_section =
            agent.thinking_avoid_echo_appendix_file.is_none();
        let inline_thinking_appendix_nonempty = agent
            .thinking_avoid_echo_appendix
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty());
        override_opt_string_non_empty(
            &mut te.thinking_avoid_echo_appendix_file,
            agent.thinking_avoid_echo_appendix_file.clone(),
        );
        if let Some(ref s) = agent.thinking_avoid_echo_appendix
            && !s.trim().is_empty()
        {
            te.thinking_avoid_echo_appendix = Some(s.clone());
        }
        if no_thinking_appendix_file_in_section && inline_thinking_appendix_nonempty {
            te.thinking_avoid_echo_appendix_file = None;
        }
        let cp = &mut self.context_pipeline;
        cp.context_char_budget = agent.context_char_budget.or(cp.context_char_budget);
        cp.context_min_messages_after_system = agent
            .context_min_messages_after_system
            .or(cp.context_min_messages_after_system);
        cp.context_summary_trigger_chars = agent
            .context_summary_trigger_chars
            .or(cp.context_summary_trigger_chars);
        cp.context_summary_tail_messages = agent
            .context_summary_tail_messages
            .or(cp.context_summary_tail_messages);
        cp.context_summary_max_tokens = agent
            .context_summary_max_tokens
            .or(cp.context_summary_max_tokens);
        cp.context_summary_transcript_max_chars = agent
            .context_summary_transcript_max_chars
            .or(cp.context_summary_transcript_max_chars);
        let wa = &mut self.web_api;
        wa.health_llm_models_probe = agent.health_llm_models_probe.or(wa.health_llm_models_probe);
        wa.health_llm_models_probe_cache_secs = agent
            .health_llm_models_probe_cache_secs
            .or(wa.health_llm_models_probe_cache_secs);
    }

    /// 队列、会话变更列表与分阶段规划字段合并。
    fn apply_section_merge_numeric_tail_queues_staged(&mut self, agent: &AgentSection) {
        let cqc = &mut self.chat_queues_cache;
        cqc.chat_queue_max_concurrent = agent
            .chat_queue_max_concurrent
            .or(cqc.chat_queue_max_concurrent);
        cqc.chat_queue_max_pending = agent.chat_queue_max_pending.or(cqc.chat_queue_max_pending);
        cqc.parallel_readonly_tools_max = agent
            .parallel_readonly_tools_max
            .or(cqc.parallel_readonly_tools_max);
        cqc.read_file_turn_cache_max_entries = agent
            .read_file_turn_cache_max_entries
            .or(cqc.read_file_turn_cache_max_entries);
        cqc.readonly_tool_ttl_cache_secs = agent
            .readonly_tool_ttl_cache_secs
            .or(cqc.readonly_tool_ttl_cache_secs);
        cqc.readonly_tool_ttl_cache_max_entries = agent
            .readonly_tool_ttl_cache_max_entries
            .or(cqc.readonly_tool_ttl_cache_max_entries);
        cqc.test_result_cache_enabled = agent
            .test_result_cache_enabled
            .or(cqc.test_result_cache_enabled);
        cqc.test_result_cache_max_entries = agent
            .test_result_cache_max_entries
            .or(cqc.test_result_cache_max_entries);
        let swc = &mut self.session_workspace_changelist;
        swc.session_workspace_changelist_enabled = agent
            .session_workspace_changelist_enabled
            .or(swc.session_workspace_changelist_enabled);
        swc.session_workspace_changelist_max_chars = agent
            .session_workspace_changelist_max_chars
            .or(swc.session_workspace_changelist_max_chars);
        let sp = &mut self.staged_planning;
        sp.staged_plan_allow_no_task = agent
            .staged_plan_allow_no_task
            .or(sp.staged_plan_allow_no_task);
        override_opt_string_non_empty(
            &mut sp.staged_plan_feedback_mode_str,
            agent.staged_plan_feedback_mode.clone(),
        );
        sp.staged_plan_patch_max_attempts = agent
            .staged_plan_patch_max_attempts
            .or(sp.staged_plan_patch_max_attempts);
        sp.staged_plan_cli_show_planner_stream = agent
            .staged_plan_cli_show_planner_stream
            .or(sp.staged_plan_cli_show_planner_stream);
        sp.staged_plan_optimizer_round = agent
            .staged_plan_optimizer_round
            .or(sp.staged_plan_optimizer_round);
        sp.staged_plan_optimizer_requires_parallel_tools = agent
            .staged_plan_optimizer_requires_parallel_tools
            .or(sp.staged_plan_optimizer_requires_parallel_tools);
        sp.staged_plan_ensemble_count = agent
            .staged_plan_ensemble_count
            .or(sp.staged_plan_ensemble_count);
        sp.staged_plan_skip_ensemble_on_casual_prompt = agent
            .staged_plan_skip_ensemble_on_casual_prompt
            .or(sp.staged_plan_skip_ensemble_on_casual_prompt);
        sp.staged_plan_two_phase_nl_display = agent
            .staged_plan_two_phase_nl_display
            .or(sp.staged_plan_two_phase_nl_display);
        sp.staged_plan_intent_gate_advisory_bypass = agent
            .staged_plan_intent_gate_advisory_bypass
            .or(sp.staged_plan_intent_gate_advisory_bypass);
        if let Some(v) = agent
            .staged_plan_advisory_bypass_extra_impl_blockers
            .as_ref()
            && !v.is_empty()
        {
            sp.staged_plan_advisory_bypass_extra_impl_blockers = Some(v.clone());
        }
        if let Some(v) = agent
            .staged_plan_advisory_bypass_extra_arch_markers
            .as_ref()
            && !v.is_empty()
        {
            sp.staged_plan_advisory_bypass_extra_arch_markers = Some(v.clone());
        }
        if let Some(v) = agent
            .staged_plan_advisory_bypass_extra_consult_markers
            .as_ref()
            && !v.is_empty()
        {
            sp.staged_plan_advisory_bypass_extra_consult_markers = Some(v.clone());
        }
        override_opt_string_non_empty(
            &mut sp.staged_plan_baseline_mode_str,
            agent.staged_plan_baseline_mode.clone(),
        );
    }

    /// 同步工具沙盒、Web API 审计与会话持久化路径合并。
    fn apply_section_merge_numeric_tail_sandbox_web_conv(&mut self, agent: &AgentSection) {
        let sb = &mut self.sync_tool_sandbox;
        override_opt_string_non_empty(
            &mut sb.sync_default_tool_sandbox_mode_str,
            agent.sync_default_tool_sandbox_mode.clone(),
        );
        override_opt_string_non_empty(
            &mut sb.sync_default_tool_sandbox_docker_image,
            agent.sync_default_tool_sandbox_docker_image.clone(),
        );
        override_opt_string_non_empty(
            &mut sb.sync_default_tool_sandbox_docker_network,
            agent.sync_default_tool_sandbox_docker_network.clone(),
        );
        sb.sync_default_tool_sandbox_docker_timeout_secs = agent
            .sync_default_tool_sandbox_docker_timeout_secs
            .or(sb.sync_default_tool_sandbox_docker_timeout_secs);
        override_opt_string_non_empty(
            &mut sb.sync_default_tool_sandbox_docker_user,
            agent.sync_default_tool_sandbox_docker_user.clone(),
        );
        let wa = &mut self.web_api;
        wa.web_api_require_bearer = agent.web_api_require_bearer.or(wa.web_api_require_bearer);
        wa.web_audit_log_write_tools = agent
            .web_audit_log_write_tools
            .or(wa.web_audit_log_write_tools);
        wa.web_audit_trust_x_forwarded_for = agent
            .web_audit_trust_x_forwarded_for
            .or(wa.web_audit_trust_x_forwarded_for);
        wa.allow_insecure_no_auth_for_non_loopback = agent
            .allow_insecure_no_auth_for_non_loopback
            .or(wa.allow_insecure_no_auth_for_non_loopback);
        override_opt_string_non_empty(
            &mut self.conversation_persistence.conversation_store_sqlite_path,
            agent.conversation_store_sqlite_path.clone(),
        );
    }

    /// 上下文引导注入与工具调用解释字段合并。
    fn apply_section_merge_numeric_tail_context_tool_explain(&mut self, agent: &AgentSection) {
        let cbi = &mut self.context_bootstrap_inject;
        cbi.agent_memory_file_enabled = agent
            .agent_memory_file_enabled
            .or(cbi.agent_memory_file_enabled);
        override_opt_string_non_empty(&mut cbi.agent_memory_file, agent.agent_memory_file.clone());
        cbi.agent_memory_file_max_chars = agent
            .agent_memory_file_max_chars
            .or(cbi.agent_memory_file_max_chars);
        cbi.living_docs_inject_enabled = agent
            .living_docs_inject_enabled
            .or(cbi.living_docs_inject_enabled);
        override_opt_string_non_empty(
            &mut cbi.living_docs_relative_dir,
            agent.living_docs_relative_dir.clone(),
        );
        cbi.living_docs_inject_max_chars = agent
            .living_docs_inject_max_chars
            .or(cbi.living_docs_inject_max_chars);
        cbi.living_docs_file_max_each_chars = agent
            .living_docs_file_max_each_chars
            .or(cbi.living_docs_file_max_each_chars);
        cbi.project_profile_inject_enabled = agent
            .project_profile_inject_enabled
            .or(cbi.project_profile_inject_enabled);
        cbi.project_profile_inject_max_chars = agent
            .project_profile_inject_max_chars
            .or(cbi.project_profile_inject_max_chars);
        cbi.project_dependency_brief_inject_enabled = agent
            .project_dependency_brief_inject_enabled
            .or(cbi.project_dependency_brief_inject_enabled);
        cbi.project_dependency_brief_inject_max_chars = agent
            .project_dependency_brief_inject_max_chars
            .or(cbi.project_dependency_brief_inject_max_chars);
        let tce = &mut self.tool_call_explain;
        tce.tool_call_explain_enabled = agent
            .tool_call_explain_enabled
            .or(tce.tool_call_explain_enabled);
        tce.tool_call_explain_min_chars = agent
            .tool_call_explain_min_chars
            .or(tce.tool_call_explain_min_chars);
        tce.tool_call_explain_max_chars = agent
            .tool_call_explain_max_chars
            .or(tce.tool_call_explain_max_chars);
    }

    /// 长期记忆、MCP、语义代码库与意图路由阈值合并。
    fn apply_section_merge_numeric_tail_memory_mcp_semantic_intent(
        &mut self,
        agent: &AgentSection,
    ) {
        let ltm = &mut self.long_term_memory;
        ltm.long_term_memory_enabled = agent
            .long_term_memory_enabled
            .or(ltm.long_term_memory_enabled);
        override_opt_string_non_empty(
            &mut ltm.long_term_memory_scope_mode_str,
            agent.long_term_memory_scope_mode.clone(),
        );
        override_opt_string_non_empty(
            &mut ltm.long_term_memory_vector_backend_str,
            agent.long_term_memory_vector_backend.clone(),
        );
        ltm.long_term_memory_max_entries = agent
            .long_term_memory_max_entries
            .or(ltm.long_term_memory_max_entries);
        ltm.long_term_memory_inject_max_chars = agent
            .long_term_memory_inject_max_chars
            .or(ltm.long_term_memory_inject_max_chars);
        override_opt_string_non_empty(
            &mut ltm.long_term_memory_store_sqlite_path,
            agent.long_term_memory_store_sqlite_path.clone(),
        );
        ltm.long_term_memory_top_k = agent.long_term_memory_top_k.or(ltm.long_term_memory_top_k);
        ltm.long_term_memory_max_chars_per_chunk = agent
            .long_term_memory_max_chars_per_chunk
            .or(ltm.long_term_memory_max_chars_per_chunk);
        ltm.long_term_memory_min_chars_to_index = agent
            .long_term_memory_min_chars_to_index
            .or(ltm.long_term_memory_min_chars_to_index);
        ltm.long_term_memory_async_index = agent
            .long_term_memory_async_index
            .or(ltm.long_term_memory_async_index);
        ltm.long_term_memory_auto_index_turns = agent
            .long_term_memory_auto_index_turns
            .or(ltm.long_term_memory_auto_index_turns);
        ltm.long_term_memory_auto_summarize_experience = agent
            .long_term_memory_auto_summarize_experience
            .or(ltm.long_term_memory_auto_summarize_experience);
        ltm.long_term_memory_prioritize_experience_recall = agent
            .long_term_memory_prioritize_experience_recall
            .or(ltm.long_term_memory_prioritize_experience_recall);
        ltm.long_term_memory_default_ttl_secs = agent
            .long_term_memory_default_ttl_secs
            .or(ltm.long_term_memory_default_ttl_secs);
        let mcp = &mut self.mcp_client;
        mcp.mcp_enabled = agent.mcp_enabled.or(mcp.mcp_enabled);
        override_opt_string_non_empty(&mut mcp.mcp_command, agent.mcp_command.clone());
        mcp.mcp_tool_timeout_secs = agent.mcp_tool_timeout_secs.or(mcp.mcp_tool_timeout_secs);
        let cs = &mut self.codebase_semantic;
        cs.codebase_semantic_search_enabled = agent
            .codebase_semantic_search_enabled
            .or(cs.codebase_semantic_search_enabled);
        cs.codebase_semantic_invalidate_on_workspace_change = agent
            .codebase_semantic_invalidate_on_workspace_change
            .or(cs.codebase_semantic_invalidate_on_workspace_change);
        override_opt_string_non_empty(
            &mut cs.codebase_semantic_index_sqlite_path,
            agent.codebase_semantic_index_sqlite_path.clone(),
        );
        cs.codebase_semantic_max_file_bytes = agent
            .codebase_semantic_max_file_bytes
            .or(cs.codebase_semantic_max_file_bytes);
        cs.codebase_semantic_chunk_max_chars = agent
            .codebase_semantic_chunk_max_chars
            .or(cs.codebase_semantic_chunk_max_chars);
        cs.codebase_semantic_top_k = agent.codebase_semantic_top_k.or(cs.codebase_semantic_top_k);
        cs.codebase_semantic_query_max_chunks = agent
            .codebase_semantic_query_max_chunks
            .or(cs.codebase_semantic_query_max_chunks);
        cs.codebase_semantic_rebuild_max_files = agent
            .codebase_semantic_rebuild_max_files
            .or(cs.codebase_semantic_rebuild_max_files);
        cs.codebase_semantic_rebuild_incremental = agent
            .codebase_semantic_rebuild_incremental
            .or(cs.codebase_semantic_rebuild_incremental);
        cs.codebase_semantic_hybrid_alpha = agent
            .codebase_semantic_hybrid_alpha
            .or(cs.codebase_semantic_hybrid_alpha);
        cs.codebase_semantic_fts_top_n = agent
            .codebase_semantic_fts_top_n
            .or(cs.codebase_semantic_fts_top_n);
        cs.codebase_semantic_hybrid_semantic_pool = agent
            .codebase_semantic_hybrid_semantic_pool
            .or(cs.codebase_semantic_hybrid_semantic_pool);
        let ir = &mut self.intent_routing;
        ir.intent_execute_low_threshold = agent
            .intent_execute_low_threshold
            .or(ir.intent_execute_low_threshold);
        ir.intent_execute_high_threshold = agent
            .intent_execute_high_threshold
            .or(ir.intent_execute_high_threshold);
        ir.intent_non_hier_execute_low_threshold = agent
            .intent_non_hier_execute_low_threshold
            .or(ir.intent_non_hier_execute_low_threshold);
        ir.intent_non_hier_execute_high_threshold = agent
            .intent_non_hier_execute_high_threshold
            .or(ir.intent_non_hier_execute_high_threshold);
        ir.intent_mode_bias_enabled = agent
            .intent_mode_bias_enabled
            .or(ir.intent_mode_bias_enabled);
        ir.intent_l2_enabled = agent.intent_l2_enabled.or(ir.intent_l2_enabled);
        ir.intent_l2_min_confidence = agent
            .intent_l2_min_confidence
            .or(ir.intent_l2_min_confidence);
        ir.intent_l2_max_tokens = agent.intent_l2_max_tokens.or(ir.intent_l2_max_tokens);
        ir.intent_at_turn_start_enabled = agent
            .intent_at_turn_start_enabled
            .or(ir.intent_at_turn_start_enabled);
        ir.intent_l0_routing_boost_enabled = agent
            .intent_l0_routing_boost_enabled
            .or(ir.intent_l0_routing_boost_enabled);
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
            if let Some(v) = row.prepend_coding_workbench {
                slot.prepend_coding_workbench = Some(v);
            }
        }
    }

    pub(super) fn merge_scheduled_agent_task_rows(&mut self, rows: &[ScheduledAgentTaskRow]) {
        for row in rows {
            self.scheduled_agent_task_rows.push(row.clone());
        }
    }

    pub(super) fn apply_tool_registry(&mut self, tr: ToolRegistrySection) {
        let p = &mut self.tool_registry_policy;
        if let Some(v) = tr.http_fetch_wall_timeout_secs {
            p.tool_registry_http_fetch_wall_timeout_secs = Some(v);
        }
        if let Some(v) = tr.http_request_wall_timeout_secs {
            p.tool_registry_http_request_wall_timeout_secs = Some(v);
        }
        for (k, v) in tr.parallel_wall_timeout_secs {
            p.tool_registry_parallel_wall_timeout_secs.insert(k, v);
        }
        if let Some(v) = tr.parallel_sync_denied_tools {
            p.tool_registry_parallel_sync_denied_tools = Some(v);
        }
        if let Some(v) = tr.parallel_sync_denied_prefixes {
            p.tool_registry_parallel_sync_denied_prefixes = Some(v);
        }
        if let Some(v) = tr.sync_default_inline_tools {
            p.tool_registry_sync_default_inline_tools = Some(v);
        }
        if let Some(v) = tr.write_effect_tools {
            p.tool_registry_write_effect_tools = Some(v);
        }
        if let Some(v) = tr.sub_agent_patch_write_extra_tools {
            p.tool_registry_sub_agent_patch_write_extra_tools = Some(v);
        }
        if let Some(v) = tr.sub_agent_test_runner_extra_tools {
            p.tool_registry_sub_agent_test_runner_extra_tools = Some(v);
        }
        if let Some(v) = tr.sub_agent_review_readonly_deny_tools {
            p.tool_registry_sub_agent_review_readonly_deny_tools = Some(v);
        }
    }
}
