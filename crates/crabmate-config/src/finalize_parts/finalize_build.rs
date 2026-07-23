fn build_agent_config_from_finalize(mid: FinalizeAfterRoles, tail: FinalizeTailScalars) -> AgentConfig {
    assemble_agent_config_from_finalize(&mid, &tail)
}

fn assemble_agent_config_from_finalize(mid: &FinalizeAfterRoles, tail: &FinalizeTailScalars) -> AgentConfig {
    AgentConfig {
        llm: finalize_section_llm_connection(mid),
        session_ui: finalize_section_session_ui(mid),
        command_exec: finalize_section_command_exec(mid),
        llm_sampling: finalize_section_llm_sampling(mid),
        llm_vendor_flags: finalize_section_llm_vendor_flags(mid),
        llm_http_retry: finalize_section_llm_http_retry(mid),
        weather_tool: finalize_section_weather_tool(mid),
        web_search: finalize_section_web_search(tail),
        http_fetch: finalize_section_http_fetch(tail),
        per_plan_policy: finalize_section_per_plan_policy(mid, tail),
        roles_prompts: finalize_section_roles_prompts(mid),
        cursor_rules: finalize_section_cursor_rules(tail),
        skills: finalize_section_skills(tail),
        tool_transcript: finalize_section_tool_transcript(tail),
        agent_thinking_trace: finalize_section_agent_thinking_trace(tail),
        agent_tool_stats: finalize_section_agent_tool_stats(tail),
        dsml_materialize: finalize_section_dsml_materialize(tail),
        thinking_echo: finalize_section_thinking_echo(tail),
        context_pipeline: finalize_section_context_pipeline(tail),
        workspace_roots: finalize_section_workspace_roots(mid),
        web_api: finalize_section_web_api(tail),
        chat_queues_cache: finalize_section_chat_queues_cache(tail),
        session_workspace_changelist: finalize_section_session_workspace_changelist(tail),
        sync_tool_sandbox: finalize_section_sync_tool_sandbox(tail),
        conversation_persistence: finalize_section_conversation_persistence(mid, tail),
        context_bootstrap_inject: finalize_section_context_bootstrap_inject(tail),
        tool_call_explain: finalize_section_tool_call_explain(tail),
        long_term_memory: finalize_section_long_term_memory(mid),
        mcp_client: finalize_section_mcp_client(tail),
        codebase_semantic: finalize_section_codebase_semantic(mid),
        tool_registry_policy: finalize_section_tool_registry_policy(mid),
        turn_budget: finalize_section_turn_budget(),
        hierarchy_routing: finalize_section_hierarchy_routing(),
        intent_routing: finalize_section_intent_routing(mid),
    }
}

fn finalize_section_llm_connection(mid: &FinalizeAfterRoles) -> types::LlmConnectionConfig {
    types::LlmConnectionConfig {
        api_base: mid.b.llm.api_base.clone(),
        model: mid.b.llm.model.clone(),
        planner_model: mid.b.llm.planner_model.clone(),
        executor_model: mid.b.llm.executor_model.clone(),
        llm_http_auth_mode: mid.intent.llm_http_auth_mode,
    }
}

fn finalize_section_session_ui(mid: &FinalizeAfterRoles) -> types::SessionUiConfig {
    types::SessionUiConfig {
        max_message_history: mid.max_message_history,
        tui_load_session_on_start: mid.tui_load_session_on_start,
        tui_session_max_messages: mid.tui_session_max_messages,
        repl_initial_workspace_messages_enabled: mid.repl_initial_workspace_messages_enabled,
    }
}

fn finalize_section_command_exec(mid: &FinalizeAfterRoles) -> types::CommandExecConfig {
    types::CommandExecConfig {
        command_timeout_secs: mid.command_timeout_secs,
        command_max_output_len: mid.command_max_output_len,
        allowed_commands: mid.allowed_commands.clone(),
        run_command_working_dir: mid.run_command_working_dir.display().to_string(),
    }
}

fn finalize_section_llm_sampling(mid: &FinalizeAfterRoles) -> types::LlmSamplingConfig {
    types::LlmSamplingConfig {
        max_tokens: mid.max_tokens,
        llm_context_tokens: mid.llm_context_tokens,
        temperature: mid.temperature,
        llm_seed: mid.b.llm_sampling.llm_seed,
    }
}

fn finalize_section_llm_vendor_flags(mid: &FinalizeAfterRoles) -> types::LlmVendorFlagsConfig {
    types::LlmVendorFlagsConfig {
        llm_reasoning_split: mid.intent.llm_reasoning_split,
        llm_bigmodel_thinking: mid.b.llm_vendor.llm_bigmodel_thinking.unwrap_or(false),
        llm_kimi_thinking_disabled: mid.b.llm_vendor.llm_kimi_thinking_disabled.unwrap_or(false),
    }
}

fn finalize_section_llm_http_retry(mid: &FinalizeAfterRoles) -> types::LlmHttpRetryConfig {
    types::LlmHttpRetryConfig {
        api_timeout_secs: mid.api_timeout_secs,
        api_max_retries: mid.api_max_retries,
        api_retry_delay_secs: mid.api_retry_delay_secs,
    }
}

fn finalize_section_weather_tool(mid: &FinalizeAfterRoles) -> types::WeatherToolConfig {
    types::WeatherToolConfig {
        weather_timeout_secs: mid.weather_timeout_secs,
    }
}

fn finalize_section_web_search(tail: &FinalizeTailScalars) -> types::WebSearchConfigSection {
    types::WebSearchConfigSection {
        web_search_provider: tail.web_search_provider,
        web_search_api_key: tail.web_search_api_key.clone(),
        web_search_timeout_secs: tail.web_search_timeout_secs,
        web_search_max_results: tail.web_search_max_results,
    }
}

fn finalize_section_http_fetch(tail: &FinalizeTailScalars) -> types::HttpFetchConfigSection {
    types::HttpFetchConfigSection {
        http_fetch_allowed_prefixes: tail.http_fetch_allowed_prefixes.clone(),
        http_fetch_timeout_secs: tail.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: tail.http_fetch_max_response_bytes,
    }
}

fn finalize_section_per_plan_policy(
    mid: &FinalizeAfterRoles,
    tail: &FinalizeTailScalars,
) -> types::PerPlanPolicyConfig {
    types::PerPlanPolicyConfig {
        reflection_default_max_rounds: mid.reflection_default_max_rounds,
        final_plan_requirement: tail.final_plan_requirement,
        plan_rewrite_max_attempts: tail.plan_rewrite_max_attempts,
        final_plan_require_strict_workflow_node_coverage: tail
            .final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled: tail.final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools: tail
            .final_plan_semantic_check_max_non_readonly_tools,
        final_plan_semantic_check_max_tokens: tail.final_plan_semantic_check_max_tokens,
        planner_executor_mode: tail.planner_executor_mode,
    }
}

fn finalize_section_roles_prompts(mid: &FinalizeAfterRoles) -> types::RolesPromptsConfig {
    types::RolesPromptsConfig {
        system_prompt: mid.system_prompt.clone(),
        default_agent_role_id: mid.default_agent_role_id.clone(),
        agent_roles: mid.agent_roles.clone(),
        coding_workbench_enabled: mid.coding_workbench_enabled,
        coding_workbench_increment_file: mid.coding_workbench_increment_file.clone(),
    }
}

fn finalize_section_cursor_rules(tail: &FinalizeTailScalars) -> types::CursorRulesConfigSection {
    types::CursorRulesConfigSection {
        cursor_rules_enabled: tail.cursor_rules_enabled,
        cursor_rules_dir: tail.cursor_rules_dir.clone(),
        cursor_rules_include_agents_md: tail.cursor_rules_include_agents_md,
        cursor_rules_max_chars: tail.cursor_rules_max_chars as usize,
    }
}

fn finalize_section_skills(tail: &FinalizeTailScalars) -> types::SkillsConfigSection {
    types::SkillsConfigSection {
        skills_enabled: tail.skills_enabled,
        skills_dir: tail.skills_dir.clone(),
        skills_max_chars: tail.skills_max_chars as usize,
        skills_top_k: tail.skills_top_k,
    }
}

fn finalize_section_tool_transcript(tail: &FinalizeTailScalars) -> types::ToolTranscriptConfig {
    types::ToolTranscriptConfig {
        tool_message_max_chars: tail.tool_message_max_chars,
        tool_result_envelope_v1: tail.tool_result_envelope_v1,
        sse_tool_call_include_arguments: tail.sse_tool_call_include_arguments,
    }
}

fn finalize_section_agent_thinking_trace(tail: &FinalizeTailScalars) -> types::AgentThinkingTraceConfig {
    types::AgentThinkingTraceConfig {
        agent_thinking_trace_enabled: tail.agent_thinking_trace_enabled,
    }
}

fn finalize_section_agent_tool_stats(tail: &FinalizeTailScalars) -> types::AgentToolStatsConfig {
    types::AgentToolStatsConfig {
        agent_tool_stats_enabled: tail.agent_tool_stats_enabled,
        agent_tool_stats_window_events: tail.agent_tool_stats_window_events,
        agent_tool_stats_min_samples: tail.agent_tool_stats_min_samples,
        agent_tool_stats_max_chars: tail.agent_tool_stats_max_chars,
        agent_tool_stats_warn_below_success_ratio: tail.agent_tool_stats_warn_below_success_ratio,
    }
}

fn finalize_section_dsml_materialize(tail: &FinalizeTailScalars) -> types::DsmlMaterializeConfig {
    types::DsmlMaterializeConfig {
        materialize_deepseek_dsml_tool_calls: tail.materialize_deepseek_dsml_tool_calls,
        dsml_stream_strip_enabled: tail.dsml_stream_strip_enabled,
    }
}

fn finalize_section_thinking_echo(tail: &FinalizeTailScalars) -> types::ThinkingEchoConfig {
    types::ThinkingEchoConfig {
        thinking_avoid_echo_system_prompt: tail.thinking_avoid_echo_system_prompt,
        thinking_avoid_echo_appendix: tail.thinking_avoid_echo_appendix.clone(),
    }
}

fn finalize_section_context_pipeline(tail: &FinalizeTailScalars) -> types::ContextPipelineConfig {
    types::ContextPipelineConfig {
        context_char_budget: tail.context_char_budget,
        context_min_messages_after_system: tail.context_min_messages_after_system,
        context_summary_trigger_chars: tail.context_summary_trigger_chars,
        context_summary_tail_messages: tail.context_summary_tail_messages,
        context_summary_max_tokens: tail.context_summary_max_tokens,
        context_summary_transcript_max_chars: tail.context_summary_transcript_max_chars,
    }
}

fn finalize_section_workspace_roots(mid: &FinalizeAfterRoles) -> types::WorkspaceRootsConfig {
    types::WorkspaceRootsConfig {
        workspace_allowed_roots: mid.workspace_allowed_roots.clone(),
    }
}

fn finalize_section_web_api(tail: &FinalizeTailScalars) -> types::WebApiConfig {
    types::WebApiConfig {
        web_api_bearer_token: tail.web_api_bearer_token.clone(),
        web_api_require_bearer: tail.web_api_require_bearer,
        web_audit_log_write_tools: tail.web_audit_log_write_tools,
        web_audit_trust_x_forwarded_for: tail.web_audit_trust_x_forwarded_for,
        allow_insecure_no_auth_for_non_loopback: tail.allow_insecure_no_auth_for_non_loopback,
        health_llm_models_probe: tail.health_llm_models_probe,
        health_llm_models_probe_cache_secs: tail.health_llm_models_probe_cache_secs,
    }
}

fn finalize_section_chat_queues_cache(tail: &FinalizeTailScalars) -> types::ChatQueuesCacheConfig {
    types::ChatQueuesCacheConfig {
        chat_queue_max_concurrent: tail.chat_queue_max_concurrent,
        chat_queue_max_pending: tail.chat_queue_max_pending,
        parallel_readonly_tools_max: tail.parallel_readonly_tools_max,
        read_file_turn_cache_max_entries: tail.read_file_turn_cache_max_entries,
        readonly_tool_ttl_cache_secs: tail.readonly_tool_ttl_cache_secs,
        readonly_tool_ttl_cache_max_entries: tail.readonly_tool_ttl_cache_max_entries,
        test_result_cache_enabled: tail.test_result_cache_enabled,
        test_result_cache_max_entries: tail.test_result_cache_max_entries,
    }
}

fn finalize_section_session_workspace_changelist(
    tail: &FinalizeTailScalars,
) -> types::SessionWorkspaceChangelistConfig {
    types::SessionWorkspaceChangelistConfig {
        session_workspace_changelist_enabled: tail.session_workspace_changelist_enabled,
        session_workspace_changelist_max_chars: tail.session_workspace_changelist_max_chars,
    }
}

fn finalize_section_sync_tool_sandbox(tail: &FinalizeTailScalars) -> types::SyncToolSandboxConfig {
    types::SyncToolSandboxConfig {
        sync_default_tool_sandbox_mode: tail.sync_default_tool_sandbox_mode,
        sync_default_tool_sandbox_docker_image: tail.sync_default_tool_sandbox_docker_image.clone(),
        sync_default_tool_sandbox_docker_network: tail.sync_default_tool_sandbox_docker_network.clone(),
        sync_default_tool_sandbox_docker_timeout_secs: tail.sync_default_tool_sandbox_docker_timeout_secs,
        sync_default_tool_sandbox_docker_user: tail.sync_default_tool_sandbox_docker_user.clone(),
    }
}

fn finalize_section_conversation_persistence(
    mid: &FinalizeAfterRoles,
    tail: &FinalizeTailScalars,
) -> types::ConversationPersistenceConfig {
    types::ConversationPersistenceConfig {
        conversation_store_sqlite_path: tail.conversation_store_sqlite_path.clone(),
        scheduled_agent_tasks: mid.scheduled_agent_tasks.clone(),
    }
}

fn finalize_section_context_bootstrap_inject(tail: &FinalizeTailScalars) -> types::ContextBootstrapInjectConfig {
    types::ContextBootstrapInjectConfig {
        agent_memory_file_enabled: tail.agent_memory_file_enabled,
        agent_memory_file: tail.agent_memory_file.clone(),
        agent_memory_file_max_chars: tail.agent_memory_file_max_chars,
        living_docs_inject_enabled: tail.living_docs_inject_enabled,
        living_docs_relative_dir: tail.living_docs_relative_dir.clone(),
        living_docs_inject_max_chars: tail.living_docs_inject_max_chars,
        living_docs_file_max_each_chars: tail.living_docs_file_max_each_chars,
        project_profile_inject_enabled: tail.project_profile_inject_enabled,
        project_profile_inject_max_chars: tail.project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled: tail.project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars: tail.project_dependency_brief_inject_max_chars,
    }
}

fn finalize_section_tool_call_explain(tail: &FinalizeTailScalars) -> types::ToolCallExplainConfig {
    types::ToolCallExplainConfig {
        tool_call_explain_enabled: tail.tool_call_explain_enabled,
        tool_call_explain_min_chars: tail.tool_call_explain_min_chars,
        tool_call_explain_max_chars: tail.tool_call_explain_max_chars,
    }
}

fn finalize_section_long_term_memory(mid: &FinalizeAfterRoles) -> types::LongTermMemoryConfig {
    let ltm = &mid.ltm;
    types::LongTermMemoryConfig {
        long_term_memory_enabled: ltm.long_term_memory_enabled,
        long_term_memory_scope_mode: ltm.long_term_memory_scope_mode,
        long_term_memory_vector_backend: ltm.long_term_memory_vector_backend,
        long_term_memory_max_entries: ltm.long_term_memory_max_entries,
        long_term_memory_inject_max_chars: ltm.long_term_memory_inject_max_chars,
        long_term_memory_store_sqlite_path: ltm.long_term_memory_store_sqlite_path.clone(),
        long_term_memory_top_k: ltm.long_term_memory_top_k,
        long_term_memory_max_chars_per_chunk: ltm.long_term_memory_max_chars_per_chunk,
        long_term_memory_min_chars_to_index: ltm.long_term_memory_min_chars_to_index,
        long_term_memory_async_index: ltm.long_term_memory_async_index,
        long_term_memory_auto_index_turns: ltm.long_term_memory_auto_index_turns,
        long_term_memory_auto_summarize_experience: ltm
            .long_term_memory_auto_summarize_experience,
        long_term_memory_prioritize_experience_recall: ltm
            .long_term_memory_prioritize_experience_recall,
        long_term_memory_default_ttl_secs: ltm.long_term_memory_default_ttl_secs,
    }
}

fn finalize_section_mcp_client(tail: &FinalizeTailScalars) -> types::McpClientConfig {
    types::McpClientConfig {
        mcp_enabled: tail.mcp_enabled,
        mcp_command: tail.mcp_command.clone(),
        mcp_tool_timeout_secs: tail.mcp_tool_timeout_secs,
    }
}

fn finalize_section_codebase_semantic(mid: &FinalizeAfterRoles) -> types::CodebaseSemanticConfig {
    let sem = &mid.sem;
    types::CodebaseSemanticConfig {
        codebase_semantic_search_enabled: sem.codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change: sem.codebase_semantic_invalidate_on_workspace_change,
        codebase_semantic_index_sqlite_path: sem.codebase_semantic_index_sqlite_path.clone(),
        codebase_semantic_max_file_bytes: sem.codebase_semantic_max_file_bytes,
        codebase_semantic_chunk_max_chars: sem.codebase_semantic_chunk_max_chars,
        codebase_semantic_top_k: sem.codebase_semantic_top_k,
        codebase_semantic_query_max_chunks: sem.codebase_semantic_query_max_chunks,
        codebase_semantic_rebuild_max_files: sem.codebase_semantic_rebuild_max_files,
        codebase_semantic_rebuild_incremental: sem.codebase_semantic_rebuild_incremental,
        codebase_semantic_hybrid_alpha: sem.codebase_semantic_hybrid_alpha,
        codebase_semantic_fts_top_n: sem.codebase_semantic_fts_top_n,
        codebase_semantic_hybrid_semantic_pool: sem.codebase_semantic_hybrid_semantic_pool,
    }
}

fn finalize_section_tool_registry_policy(mid: &FinalizeAfterRoles) -> types::ToolRegistryPolicyConfig {
    let tr = &mid.tr;
    types::ToolRegistryPolicyConfig {
        tool_registry_http_fetch_wall_timeout_secs: tr.tool_registry_http_fetch_wall_timeout_secs,
        tool_registry_http_request_wall_timeout_secs: tr.tool_registry_http_request_wall_timeout_secs,
        tool_registry_parallel_wall_timeout_secs: tr.tool_registry_parallel_wall_timeout_secs.clone(),
        tool_registry_parallel_sync_denied_tools: tr.tool_registry_parallel_sync_denied_tools.clone(),
        tool_registry_parallel_sync_denied_prefixes: tr.tool_registry_parallel_sync_denied_prefixes.clone(),
        tool_registry_sync_default_inline_tools: tr.tool_registry_sync_default_inline_tools.clone(),
        tool_registry_write_effect_tools: tr.tool_registry_write_effect_tools.clone(),
        tool_registry_sub_agent_patch_write_extra_tools: tr.tool_registry_sub_agent_patch_write_extra_tools.clone(),
        tool_registry_sub_agent_test_runner_extra_tools: tr.tool_registry_sub_agent_test_runner_extra_tools.clone(),
        tool_registry_sub_agent_review_readonly_deny_tools: tr.tool_registry_sub_agent_review_readonly_deny_tools.clone(),
    }
}

fn finalize_section_turn_budget() -> types::TurnBudgetConfig {
    let mut cfg = types::TurnBudgetConfig {
        max_turn_duration_seconds: 600,
        max_turn_tokens: 0,
        max_llm_calls_per_turn: 0,
        max_outer_loop_iterations: 0,
        full_plan_rewrite_max_attempts: 2,
        budget_degradation_enabled: false,
        budget_degradation_threshold_percent: 80,
    };
    if let Ok(v) = std::env::var("CM_MAX_TURN_DURATION_SECONDS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        cfg.max_turn_duration_seconds = n;
    }
    if let Ok(v) = std::env::var("CM_MAX_TURN_TOKENS")
        && let Ok(n) = v.trim().parse::<usize>()
    {
        cfg.max_turn_tokens = n;
    }
    if let Ok(v) = std::env::var("CM_MAX_LLM_CALLS_PER_TURN")
        && let Ok(n) = v.trim().parse::<u32>()
    {
        cfg.max_llm_calls_per_turn = n;
    }
    if let Ok(v) = std::env::var("CM_TURN_BUDGET_DEGRADATION_ENABLED")
        && let Some(b) = crate::source::parse_bool_like(&v)
    {
        cfg.budget_degradation_enabled = b;
    }
    if let Ok(v) = std::env::var("CM_TURN_BUDGET_DEGRADATION_THRESHOLD_PERCENT")
        && let Ok(n) = v.trim().parse::<u8>()
    {
        cfg.budget_degradation_threshold_percent = n.clamp(50, 99);
    }
    cfg
}

fn finalize_section_hierarchy_routing() -> types::HierarchyRoutingConfig {
    types::HierarchyRoutingConfig {
        enable_llm_routing: Some(true), // 默认开启 LLM 智能路由
    }
}

fn finalize_section_intent_routing(mid: &FinalizeAfterRoles) -> types::IntentRoutingConfig {
    let intent = &mid.intent;
    types::IntentRoutingConfig {
        intent_mode_bias_enabled: intent.intent_mode_bias_enabled,
        intent_l2_min_confidence: intent.intent_l2_min_confidence,
        intent_l2_max_tokens: intent.intent_l2_max_tokens,
        intent_execute_low_threshold: intent.intent_execute_low_threshold,
        intent_execute_high_threshold: intent.intent_execute_high_threshold,
        intent_non_hier_execute_low_threshold: intent.intent_non_hier_execute_low_threshold,
        intent_non_hier_execute_high_threshold: intent.intent_non_hier_execute_high_threshold,
        intent_l0_routing_boost_enabled: intent.intent_l0_routing_boost_enabled,
    }
}

fn finalize_agent_config_tail(mid: FinalizeAfterRoles) -> Result<AgentConfig, String> {
    let tail = derive_finalize_tail_scalars(&mid)?;
    Ok(build_agent_config_from_finalize(mid, tail))
}

pub(super) fn finalize(
    b: ConfigBuilder,
    system_prompt_search_bases: Vec<PathBuf>,
) -> Result<AgentConfig, String> {
    finalize_agent_config(b, system_prompt_search_bases)
}
