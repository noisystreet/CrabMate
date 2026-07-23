/// `finalize_agent_config_tail` 中解析与 clamp 后的标量字段（降低单函数 `nloc`）。
#[allow(clippy::struct_excessive_bools)]
struct FinalizeTailScalars {
    cursor_rules_enabled: bool,
    cursor_rules_dir: String,
    cursor_rules_include_agents_md: bool,
    cursor_rules_max_chars: u64,
    skills_enabled: bool,
    skills_dir: String,
    skills_max_chars: u64,
    skills_top_k: usize,
    final_plan_requirement: FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    final_plan_require_strict_workflow_node_coverage: bool,
    final_plan_semantic_check_enabled: bool,
    final_plan_semantic_check_max_non_readonly_tools: usize,
    final_plan_semantic_check_max_tokens: u32,
    planner_executor_mode: PlannerExecutorMode,
    tool_message_max_chars: usize,
    tool_result_envelope_v1: bool,
    sse_tool_call_include_arguments: bool,
    agent_thinking_trace_enabled: bool,
    agent_tool_stats_enabled: bool,
    agent_tool_stats_window_events: usize,
    agent_tool_stats_min_samples: usize,
    agent_tool_stats_max_chars: usize,
    agent_tool_stats_warn_below_success_ratio: f64,
    materialize_deepseek_dsml_tool_calls: bool,
    dsml_stream_strip_enabled: bool,
    thinking_avoid_echo_system_prompt: bool,
    thinking_avoid_echo_appendix: String,
    context_char_budget: usize,
    context_min_messages_after_system: usize,
    context_summary_trigger_chars: usize,
    context_summary_tail_messages: usize,
    context_summary_max_tokens: u32,
    context_summary_transcript_max_chars: usize,
    health_llm_models_probe: bool,
    health_llm_models_probe_cache_secs: u64,
    chat_queue_max_concurrent: usize,
    chat_queue_max_pending: usize,
    parallel_readonly_tools_max: usize,
    read_file_turn_cache_max_entries: usize,
    readonly_tool_ttl_cache_secs: u64,
    readonly_tool_ttl_cache_max_entries: usize,
    test_result_cache_enabled: bool,
    test_result_cache_max_entries: usize,
    session_workspace_changelist_enabled: bool,
    session_workspace_changelist_max_chars: usize,
    sync_default_tool_sandbox_mode: types::SyncDefaultToolSandboxMode,
    sync_default_tool_sandbox_docker_image: String,
    sync_default_tool_sandbox_docker_network: String,
    sync_default_tool_sandbox_docker_timeout_secs: u64,
    sync_default_tool_sandbox_docker_user: types::SandboxDockerContainerUser,
    web_api_bearer_token: types::SecretString,
    web_api_require_bearer: bool,
    web_audit_log_write_tools: bool,
    web_audit_trust_x_forwarded_for: bool,
    allow_insecure_no_auth_for_non_loopback: bool,
    conversation_store_sqlite_path: String,
    agent_memory_file_enabled: bool,
    agent_memory_file: String,
    agent_memory_file_max_chars: usize,
    living_docs_inject_enabled: bool,
    living_docs_relative_dir: String,
    living_docs_inject_max_chars: usize,
    living_docs_file_max_each_chars: usize,
    project_profile_inject_enabled: bool,
    project_profile_inject_max_chars: usize,
    project_dependency_brief_inject_enabled: bool,
    project_dependency_brief_inject_max_chars: usize,
    tool_call_explain_enabled: bool,
    tool_call_explain_min_chars: usize,
    tool_call_explain_max_chars: usize,
    mcp_enabled: bool,
    mcp_command: String,
    mcp_tool_timeout_secs: u64,
    web_search_provider: WebSearchProvider,
    web_search_api_key: types::SecretString,
    web_search_timeout_secs: u64,
    web_search_max_results: u32,
    http_fetch_allowed_prefixes: Vec<String>,
    http_fetch_timeout_secs: u64,
    http_fetch_max_response_bytes: usize,
}

/// `derive_finalize_tail_scalars` 中段：规划/工具/思维链附录（降低单函数 `nloc`）。
#[allow(clippy::struct_excessive_bools)]
struct TailPlanToolThinkingScalars {
    final_plan_requirement: FinalPlanRequirementMode,
    plan_rewrite_max_attempts: usize,
    final_plan_require_strict_workflow_node_coverage: bool,
    final_plan_semantic_check_enabled: bool,
    final_plan_semantic_check_max_non_readonly_tools: usize,
    final_plan_semantic_check_max_tokens: u32,
    planner_executor_mode: PlannerExecutorMode,
    tool_message_max_chars: usize,
    tool_result_envelope_v1: bool,
    sse_tool_call_include_arguments: bool,
    agent_thinking_trace_enabled: bool,
    agent_tool_stats_enabled: bool,
    agent_tool_stats_window_events: usize,
    agent_tool_stats_min_samples: usize,
    agent_tool_stats_max_chars: usize,
    agent_tool_stats_warn_below_success_ratio: f64,
    materialize_deepseek_dsml_tool_calls: bool,
    dsml_stream_strip_enabled: bool,
    thinking_avoid_echo_system_prompt: bool,
    thinking_avoid_echo_appendix: String,
}

fn derive_tail_plan_tool_thinking_scalars(
    b: &ConfigBuilder,
    system_prompt_search_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<TailPlanToolThinkingScalars, String> {
    let final_plan_requirement = match b.per_plan_policy.final_plan_requirement_str.as_deref() {
        Some(s) => FinalPlanRequirementMode::parse(s)?,
        None => FinalPlanRequirementMode::default(),
    };
    let plan_rewrite_max_attempts = b.per_plan_policy.plan_rewrite_max_attempts.unwrap_or(2).clamp(1, 20) as usize;
    let final_plan_require_strict_workflow_node_coverage = b
        .per_plan_policy.final_plan_require_strict_workflow_node_coverage
        .unwrap_or(false);
    let final_plan_semantic_check_enabled = b.per_plan_policy.final_plan_semantic_check_enabled.unwrap_or(false);
    let final_plan_semantic_check_max_non_readonly_tools = b
        .per_plan_policy.final_plan_semantic_check_max_non_readonly_tools
        .unwrap_or(0)
        .min(32) as usize;
    let final_plan_semantic_check_max_tokens = b
        .per_plan_policy.final_plan_semantic_check_max_tokens
        .unwrap_or(256)
        .clamp(32, 1024) as u32;
    // 统一强制走 ReAct（单 Agent 外循环），不再暴露给用户选择。
    // `planner_executor_mode` 的 TOML/环境变量配置不再生效。
    let planner_executor_mode = PlannerExecutorMode::SingleAgent;
    let tool_message_max_chars = b
        .tool_transcript.tool_message_max_chars
        .unwrap_or(32768)
        .clamp(1024, 1_048_576) as usize;
    let tool_result_envelope_v1 = b.tool_transcript.tool_result_envelope_v1.unwrap_or(true);
    let sse_tool_call_include_arguments = b.tool_transcript.sse_tool_call_include_arguments.unwrap_or(true);
    // 默认开启；仅 `CM_THINKING_TRACE_ENABLED` 可关闭（不从 `[agent]` TOML 读入）。
    let agent_thinking_trace_enabled = b.agent_thinking_trace.agent_thinking_trace_enabled.unwrap_or(true);
    let agent_tool_stats_enabled = b.agent_tool_stats.agent_tool_stats_enabled.unwrap_or(false);
    let agent_tool_stats_window_events = b
        .agent_tool_stats.agent_tool_stats_window_events
        .unwrap_or(200)
        .clamp(16, 65_536) as usize;
    let agent_tool_stats_min_samples =
        b.agent_tool_stats.agent_tool_stats_min_samples.unwrap_or(5).clamp(1, 10_000) as usize;
    let agent_tool_stats_max_chars = b
        .agent_tool_stats.agent_tool_stats_max_chars
        .unwrap_or(800)
        .clamp(64, 32_768) as usize;
    let agent_tool_stats_warn_below_success_ratio = b
        .agent_tool_stats.agent_tool_stats_warn_below_success_ratio
        .unwrap_or(0.65)
        .clamp(0.0, 1.0);
    let materialize_deepseek_dsml_tool_calls =
        b.dsml_materialize.materialize_deepseek_dsml_tool_calls.unwrap_or(true);
    let dsml_stream_strip_enabled = b
        .dsml_materialize
        .dsml_stream_strip_enabled
        .or(b.dsml_materialize.materialize_deepseek_dsml_tool_calls)
        .unwrap_or(true);
    let thinking_avoid_echo_system_prompt = b.thinking_echo.thinking_avoid_echo_system_prompt.unwrap_or(true);
    let thinking_avoid_echo_appendix = resolve_thinking_avoid_echo_appendix(
        thinking_avoid_echo_system_prompt,
        b.thinking_echo.thinking_avoid_echo_appendix.as_deref(),
        b.thinking_echo.thinking_avoid_echo_appendix_file.as_deref(),
        system_prompt_search_bases,
        run_command_working_dir,
    )?;
    Ok(TailPlanToolThinkingScalars {
        final_plan_requirement,
        plan_rewrite_max_attempts,
        final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools,
        final_plan_semantic_check_max_tokens,
        planner_executor_mode,
        tool_message_max_chars,
        tool_result_envelope_v1,
        sse_tool_call_include_arguments,
        agent_thinking_trace_enabled,
        agent_tool_stats_enabled,
        agent_tool_stats_window_events,
        agent_tool_stats_min_samples,
        agent_tool_stats_max_chars,
        agent_tool_stats_warn_below_success_ratio,
        materialize_deepseek_dsml_tool_calls,
        dsml_stream_strip_enabled,
        thinking_avoid_echo_system_prompt,
        thinking_avoid_echo_appendix,
    })
}

#[allow(clippy::struct_excessive_bools)]
struct TailContextQueuesSessionScalars {
    context_char_budget: usize,
    context_min_messages_after_system: usize,
    context_summary_trigger_chars: usize,
    context_summary_tail_messages: usize,
    context_summary_max_tokens: u32,
    context_summary_transcript_max_chars: usize,
    health_llm_models_probe: bool,
    health_llm_models_probe_cache_secs: u64,
    chat_queue_max_concurrent: usize,
    chat_queue_max_pending: usize,
    parallel_readonly_tools_max: usize,
    read_file_turn_cache_max_entries: usize,
    readonly_tool_ttl_cache_secs: u64,
    readonly_tool_ttl_cache_max_entries: usize,
    test_result_cache_enabled: bool,
    test_result_cache_max_entries: usize,
    session_workspace_changelist_enabled: bool,
    session_workspace_changelist_max_chars: usize,
}

fn derive_tail_context_queues_session_scalars(
    b: &ConfigBuilder,
    max_message_history: usize,
) -> TailContextQueuesSessionScalars {
    let context_char_budget = b.context_pipeline.context_char_budget.unwrap_or(0).min(50_000_000) as usize;
    let context_min_messages_after_system = b
        .context_pipeline.context_min_messages_after_system
        .unwrap_or(4)
        .clamp(1, 128) as usize;
    if context_budget_vs_history_suspicious(
        max_message_history,
        context_char_budget,
        context_min_messages_after_system,
    ) {
        log::warn!(
            target: "crabmate",
            "配置提示：已启用 context_char_budget，但 context_min_messages_after_system({}) >= max_message_history({})：条数裁剪后消息条数通常不超过 1+max_message_history，按字符删旧消息往往无法生效或空间极小。建议调小 context_min_messages_after_system 或增大 max_message_history。",
            context_min_messages_after_system,
            max_message_history
        );
    }
    let context_summary_trigger_chars =
        b.context_pipeline.context_summary_trigger_chars.unwrap_or(0).min(50_000_000) as usize;
    let context_summary_tail_messages =
        b.context_pipeline.context_summary_tail_messages.unwrap_or(12).clamp(4, 64) as usize;
    let context_summary_max_tokens = b
        .context_pipeline.context_summary_max_tokens
        .unwrap_or(1024)
        .clamp(256, 8192) as u32;
    let context_summary_transcript_max_chars = b
        .context_pipeline.context_summary_transcript_max_chars
        .unwrap_or(120_000)
        .clamp(10_000, 2_000_000) as usize;
    let health_llm_models_probe = b.web_api.health_llm_models_probe.unwrap_or(false);
    let health_llm_models_probe_cache_secs = b
        .web_api.health_llm_models_probe_cache_secs
        .unwrap_or(120)
        .clamp(5, 86_400);
    let chat_queue_max_concurrent = b.chat_queues_cache.chat_queue_max_concurrent.unwrap_or(2).clamp(1, 256) as usize;
    let chat_queue_max_pending = b.chat_queues_cache.chat_queue_max_pending.unwrap_or(32).clamp(1, 8192) as usize;
    let parallel_readonly_tools_max = b
        .chat_queues_cache.parallel_readonly_tools_max
        .map(|n| n as usize)
        .unwrap_or_else(|| chat_queue_max_concurrent.max(3))
        .clamp(1, 256);
    let read_file_turn_cache_max_entries =
        b.chat_queues_cache.read_file_turn_cache_max_entries.unwrap_or(64).min(4096) as usize;
    let readonly_tool_ttl_cache_secs = b
        .chat_queues_cache
        .readonly_tool_ttl_cache_secs
        .unwrap_or(30)
        .min(3600);
    let readonly_tool_ttl_cache_max_entries = b
        .chat_queues_cache
        .readonly_tool_ttl_cache_max_entries
        .unwrap_or(256)
        .clamp(1, 4096) as usize;
    let test_result_cache_enabled = b.chat_queues_cache.test_result_cache_enabled.unwrap_or(true);
    let test_result_cache_max_entries =
        b.chat_queues_cache.test_result_cache_max_entries.unwrap_or(32).clamp(1, 512) as usize;
    let session_workspace_changelist_enabled =
        b.session_workspace_changelist.session_workspace_changelist_enabled.unwrap_or(true);
    let session_workspace_changelist_max_chars_raw =
        b.session_workspace_changelist.session_workspace_changelist_max_chars.unwrap_or(12_000);
    let session_workspace_changelist_max_chars = if session_workspace_changelist_max_chars_raw == 0
    {
        12_000usize
    } else {
        session_workspace_changelist_max_chars_raw.clamp(2_048, 500_000) as usize
    };
    TailContextQueuesSessionScalars {
        context_char_budget,
        context_min_messages_after_system,
        context_summary_trigger_chars,
        context_summary_tail_messages,
        context_summary_max_tokens,
        context_summary_transcript_max_chars,
        health_llm_models_probe,
        health_llm_models_probe_cache_secs,
        chat_queue_max_concurrent,
        chat_queue_max_pending,
        parallel_readonly_tools_max,
        read_file_turn_cache_max_entries,
        readonly_tool_ttl_cache_secs,
        readonly_tool_ttl_cache_max_entries,
        test_result_cache_enabled,
        test_result_cache_max_entries,
        session_workspace_changelist_enabled,
        session_workspace_changelist_max_chars,
    }
}

#[allow(clippy::struct_excessive_bools)]
struct TailSandboxWebScalars {
    sync_default_tool_sandbox_mode: types::SyncDefaultToolSandboxMode,
    sync_default_tool_sandbox_docker_image: String,
    sync_default_tool_sandbox_docker_network: String,
    sync_default_tool_sandbox_docker_timeout_secs: u64,
    sync_default_tool_sandbox_docker_user: types::SandboxDockerContainerUser,
    web_api_bearer_token: types::SecretString,
    web_api_require_bearer: bool,
    web_audit_log_write_tools: bool,
    web_audit_trust_x_forwarded_for: bool,
    allow_insecure_no_auth_for_non_loopback: bool,
}

fn derive_tail_sandbox_web_scalars(
    b: &ConfigBuilder,
) -> Result<TailSandboxWebScalars, String> {
    let sync_default_tool_sandbox_mode = match b.sync_tool_sandbox.sync_default_tool_sandbox_mode_str.as_deref() {
        Some(s) => types::SyncDefaultToolSandboxMode::parse(s)?,
        None => types::SyncDefaultToolSandboxMode::default(),
    };
    #[cfg(not(feature = "docker_sandbox"))]
    if sync_default_tool_sandbox_mode == types::SyncDefaultToolSandboxMode::Docker {
        return Err(
            "配置错误：当前二进制未启用 `docker_sandbox` Cargo feature，不支持 sync_default_tool_sandbox_mode=docker；请改为 none 或使用带 docker_sandbox 的构建"
                .to_string(),
        );
    }
    let sync_default_tool_sandbox_docker_image = b
        .sync_tool_sandbox.sync_default_tool_sandbox_docker_image
        .clone()
        .unwrap_or_default();
    let sync_default_tool_sandbox_docker_network = b
        .sync_tool_sandbox.sync_default_tool_sandbox_docker_network
        .clone()
        .unwrap_or_default();
    let sync_default_tool_sandbox_docker_timeout_secs = b
        .sync_tool_sandbox.sync_default_tool_sandbox_docker_timeout_secs
        .unwrap_or(600)
        .max(1);
    let sync_default_tool_sandbox_docker_user =
        types::SandboxDockerContainerUser::resolve_from_config_str(
            b.sync_tool_sandbox.sync_default_tool_sandbox_docker_user
                .as_deref()
                .unwrap_or(""),
        );
    validate_docker_sandbox_image(
        sync_default_tool_sandbox_mode,
        sync_default_tool_sandbox_docker_image.as_str(),
    )?;
    let web_api_bearer_token =
        types::SecretString::new(b.web_api.web_api_bearer_token.clone().unwrap_or_default().into());
    // 默认 **false**：允许无密钥启动 `serve`；生产环境请显式 `web_api_require_bearer = true` 并配置非空密钥。
    let web_api_require_bearer = b.web_api.web_api_require_bearer.unwrap_or(false);
    let web_audit_log_write_tools = b.web_api.web_audit_log_write_tools.unwrap_or(true);
    let web_audit_trust_x_forwarded_for = b.web_api.web_audit_trust_x_forwarded_for.unwrap_or(false);
    let allow_insecure_no_auth_for_non_loopback =
        b.web_api.allow_insecure_no_auth_for_non_loopback.unwrap_or(false);
    Ok(TailSandboxWebScalars {
        sync_default_tool_sandbox_mode,
        sync_default_tool_sandbox_docker_image,
        sync_default_tool_sandbox_docker_network,
        sync_default_tool_sandbox_docker_timeout_secs,
        sync_default_tool_sandbox_docker_user,
        web_api_bearer_token,
        web_api_require_bearer,
        web_audit_log_write_tools,
        web_audit_trust_x_forwarded_for,
        allow_insecure_no_auth_for_non_loopback,
    })
}

#[allow(clippy::struct_excessive_bools)]
struct TailStorageInjectNetScalars {
    conversation_store_sqlite_path: String,
    agent_memory_file_enabled: bool,
    agent_memory_file: String,
    agent_memory_file_max_chars: usize,
    living_docs_inject_enabled: bool,
    living_docs_relative_dir: String,
    living_docs_inject_max_chars: usize,
    living_docs_file_max_each_chars: usize,
    project_profile_inject_enabled: bool,
    project_profile_inject_max_chars: usize,
    project_dependency_brief_inject_enabled: bool,
    project_dependency_brief_inject_max_chars: usize,
    tool_call_explain_enabled: bool,
    tool_call_explain_min_chars: usize,
    tool_call_explain_max_chars: usize,
    mcp_enabled: bool,
    mcp_command: String,
    mcp_tool_timeout_secs: u64,
    web_search_provider: WebSearchProvider,
    web_search_api_key: types::SecretString,
    web_search_timeout_secs: u64,
    web_search_max_results: u32,
    http_fetch_allowed_prefixes: Vec<String>,
    http_fetch_timeout_secs: u64,
    http_fetch_max_response_bytes: usize,
}

fn derive_tail_storage_inject_net_scalars(
    b: &ConfigBuilder,
    command_timeout_secs: u64,
) -> Result<TailStorageInjectNetScalars, String> {
    let conversation_store_sqlite_path =
        b.conversation_persistence.conversation_store_sqlite_path.clone().unwrap_or_default();
    let agent_memory_file_enabled = b.context_bootstrap_inject.agent_memory_file_enabled.unwrap_or(false);
    let agent_memory_file = b
        .context_bootstrap_inject.agent_memory_file
        .clone()
        .unwrap_or_else(|| ".crabmate/agent_memory.md".to_string());
    let agent_memory_file_max_chars = b
        .context_bootstrap_inject.agent_memory_file_max_chars
        .unwrap_or(8000)
        .clamp(256, 500_000) as usize;
    let living_docs_inject_enabled = b.context_bootstrap_inject.living_docs_inject_enabled.unwrap_or(false);
    let living_docs_relative_dir = b
        .context_bootstrap_inject.living_docs_relative_dir
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| ".crabmate/living_docs".to_string());
    let living_docs_inject_max_chars = b
        .context_bootstrap_inject.living_docs_inject_max_chars
        .unwrap_or(4000)
        .clamp(0, 500_000) as usize;
    let living_docs_file_max_each_chars = b
        .context_bootstrap_inject.living_docs_file_max_each_chars
        .unwrap_or(1200)
        .clamp(0, 500_000) as usize;
    let project_profile_inject_enabled = b.context_bootstrap_inject.project_profile_inject_enabled.unwrap_or(true);
    let project_profile_inject_max_chars = b
        .context_bootstrap_inject.project_profile_inject_max_chars
        .unwrap_or(6000)
        .clamp(0, 500_000) as usize;
    let project_dependency_brief_inject_enabled =
        b.context_bootstrap_inject.project_dependency_brief_inject_enabled.unwrap_or(true);
    let project_dependency_brief_inject_max_chars = b
        .context_bootstrap_inject.project_dependency_brief_inject_max_chars
        .unwrap_or(4000)
        .clamp(0, 500_000) as usize;
    let tool_call_explain_enabled = b.tool_call_explain.tool_call_explain_enabled.unwrap_or(false);
    let tool_call_explain_min_chars =
        b.tool_call_explain.tool_call_explain_min_chars.unwrap_or(8).clamp(1, 256) as usize;
    let max_chars_raw = b.tool_call_explain.tool_call_explain_max_chars.unwrap_or(400).clamp(1, 4000) as usize;
    let tool_call_explain_max_chars = max_chars_raw.max(tool_call_explain_min_chars);

    let mcp_enabled = b.mcp_client.mcp_enabled.unwrap_or(false);
    let mcp_command = b.mcp_client.mcp_command.clone().unwrap_or_default();
    let mcp_tool_timeout_secs = b
        .mcp_client.mcp_tool_timeout_secs
        .unwrap_or(command_timeout_secs)
        .max(1);

    let web_search_provider = match b.web_search.web_search_provider_str.as_deref() {
        Some(s) => WebSearchProvider::parse(s)?,
        None => WebSearchProvider::default(),
    };
    let web_search_api_key =
        types::SecretString::new(b.web_search.web_search_api_key.clone().unwrap_or_default().into());
    let web_search_timeout_secs = b.web_search.web_search_timeout_secs.unwrap_or(30).max(1);
    let web_search_max_results = b.web_search.web_search_max_results.unwrap_or(8).clamp(1, 20) as u32;

    let http_fetch_allowed_prefixes = b.http_fetch.http_fetch_allowed_prefixes.clone().unwrap_or_default();
    let http_fetch_timeout_secs = b.http_fetch.http_fetch_timeout_secs.unwrap_or(30).max(1);
    let http_fetch_max_response_bytes = b
        .http_fetch.http_fetch_max_response_bytes
        .unwrap_or(524_288)
        .clamp(1024, 4_194_304) as usize;

    Ok(TailStorageInjectNetScalars {
        conversation_store_sqlite_path,
        agent_memory_file_enabled,
        agent_memory_file,
        agent_memory_file_max_chars,
        living_docs_inject_enabled,
        living_docs_relative_dir,
        living_docs_inject_max_chars,
        living_docs_file_max_each_chars,
        project_profile_inject_enabled,
        project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars,
        tool_call_explain_enabled,
        tool_call_explain_min_chars,
        tool_call_explain_max_chars,
        mcp_enabled,
        mcp_command,
        mcp_tool_timeout_secs,
        web_search_provider,
        web_search_api_key,
        web_search_timeout_secs,
        web_search_max_results,
        http_fetch_allowed_prefixes,
        http_fetch_timeout_secs,
        http_fetch_max_response_bytes,
    })
}

#[allow(clippy::struct_excessive_bools)]
struct TailCursorSkillsPack {
    cursor_rules_enabled: bool,
    cursor_rules_dir: String,
    cursor_rules_include_agents_md: bool,
    cursor_rules_max_chars: u64,
    skills_enabled: bool,
    skills_dir: String,
    skills_max_chars: u64,
    skills_top_k: usize,
}

fn tail_cursor_rules_and_skills_fields(b: &ConfigBuilder) -> TailCursorSkillsPack {
    let cursor_rules_enabled = b.cursor_rules.cursor_rules_enabled.unwrap_or(true);
    let cursor_rules_dir = b
        .cursor_rules.cursor_rules_dir
        .clone()
        .unwrap_or_else(|| ".cursor/rules".to_string());
    let cursor_rules_include_agents_md = b.cursor_rules.cursor_rules_include_agents_md.unwrap_or(true);
    let cursor_rules_max_chars = b
        .cursor_rules.cursor_rules_max_chars
        .unwrap_or(48_000)
        .clamp(1024, 1_000_000);
    let skills_enabled = b.skills.skills_enabled.unwrap_or(true);
    let skills_dir = b
        .skills.skills_dir
        .clone()
        .unwrap_or_else(|| ".crabmate/skills".to_string());
    let skills_max_chars = b.skills.skills_max_chars.unwrap_or(32_000).clamp(1024, 1_000_000);
    let skills_top_k = b.skills.skills_top_k.unwrap_or(4).clamp(1, 64) as usize;
    TailCursorSkillsPack {
        cursor_rules_enabled,
        cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars,
        skills_enabled,
        skills_dir,
        skills_max_chars,
        skills_top_k,
    }
}

#[allow(clippy::too_many_lines)]
fn assemble_finalize_tail_scalars(
    pack: TailCursorSkillsPack,
    ptt: TailPlanToolThinkingScalars,
    cqs: TailContextQueuesSessionScalars,
    ssw: TailSandboxWebScalars,
    sin: TailStorageInjectNetScalars,
) -> FinalizeTailScalars {
    let TailPlanToolThinkingScalars {
        final_plan_requirement,
        plan_rewrite_max_attempts,
        final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools,
        final_plan_semantic_check_max_tokens,
        planner_executor_mode,
        tool_message_max_chars,
        tool_result_envelope_v1,
        sse_tool_call_include_arguments,
        agent_thinking_trace_enabled,
        agent_tool_stats_enabled,
        agent_tool_stats_window_events,
        agent_tool_stats_min_samples,
        agent_tool_stats_max_chars,
        agent_tool_stats_warn_below_success_ratio,
        materialize_deepseek_dsml_tool_calls,
        dsml_stream_strip_enabled,
        thinking_avoid_echo_system_prompt,
        thinking_avoid_echo_appendix,
    } = ptt;
    let TailContextQueuesSessionScalars {
        context_char_budget,
        context_min_messages_after_system,
        context_summary_trigger_chars,
        context_summary_tail_messages,
        context_summary_max_tokens,
        context_summary_transcript_max_chars,
        health_llm_models_probe,
        health_llm_models_probe_cache_secs,
        chat_queue_max_concurrent,
        chat_queue_max_pending,
        parallel_readonly_tools_max,
        read_file_turn_cache_max_entries,
        readonly_tool_ttl_cache_secs,
        readonly_tool_ttl_cache_max_entries,
        test_result_cache_enabled,
        test_result_cache_max_entries,
        session_workspace_changelist_enabled,
        session_workspace_changelist_max_chars,
    } = cqs;

    FinalizeTailScalars {
        cursor_rules_enabled: pack.cursor_rules_enabled,
        cursor_rules_dir: pack.cursor_rules_dir,
        cursor_rules_include_agents_md: pack.cursor_rules_include_agents_md,
        cursor_rules_max_chars: pack.cursor_rules_max_chars,
        skills_enabled: pack.skills_enabled,
        skills_dir: pack.skills_dir,
        skills_max_chars: pack.skills_max_chars,
        skills_top_k: pack.skills_top_k,
        final_plan_requirement,
        plan_rewrite_max_attempts,
        final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools,
        final_plan_semantic_check_max_tokens,
        planner_executor_mode,
        tool_message_max_chars,
        tool_result_envelope_v1,
        sse_tool_call_include_arguments,
        agent_thinking_trace_enabled,
        agent_tool_stats_enabled,
        agent_tool_stats_window_events,
        agent_tool_stats_min_samples,
        agent_tool_stats_max_chars,
        agent_tool_stats_warn_below_success_ratio,
        materialize_deepseek_dsml_tool_calls,
        dsml_stream_strip_enabled,
        thinking_avoid_echo_system_prompt,
        thinking_avoid_echo_appendix,
        context_char_budget,
        context_min_messages_after_system,
        context_summary_trigger_chars,
        context_summary_tail_messages,
        context_summary_max_tokens,
        context_summary_transcript_max_chars,
        health_llm_models_probe,
        health_llm_models_probe_cache_secs,
        chat_queue_max_concurrent,
        chat_queue_max_pending,
        parallel_readonly_tools_max,
        read_file_turn_cache_max_entries,
        readonly_tool_ttl_cache_secs,
        readonly_tool_ttl_cache_max_entries,
        test_result_cache_enabled,
        test_result_cache_max_entries,
        session_workspace_changelist_enabled,
        session_workspace_changelist_max_chars,
        sync_default_tool_sandbox_mode: ssw.sync_default_tool_sandbox_mode,
        sync_default_tool_sandbox_docker_image: ssw.sync_default_tool_sandbox_docker_image.clone(),
        sync_default_tool_sandbox_docker_network: ssw.sync_default_tool_sandbox_docker_network.clone(),
        sync_default_tool_sandbox_docker_timeout_secs: ssw.sync_default_tool_sandbox_docker_timeout_secs,
        sync_default_tool_sandbox_docker_user: ssw.sync_default_tool_sandbox_docker_user,
        web_api_bearer_token: ssw.web_api_bearer_token.clone(),
        web_api_require_bearer: ssw.web_api_require_bearer,
        web_audit_log_write_tools: ssw.web_audit_log_write_tools,
        web_audit_trust_x_forwarded_for: ssw.web_audit_trust_x_forwarded_for,
        allow_insecure_no_auth_for_non_loopback: ssw.allow_insecure_no_auth_for_non_loopback,
        conversation_store_sqlite_path: sin.conversation_store_sqlite_path,
        agent_memory_file_enabled: sin.agent_memory_file_enabled,
        agent_memory_file: sin.agent_memory_file,
        agent_memory_file_max_chars: sin.agent_memory_file_max_chars,
        living_docs_inject_enabled: sin.living_docs_inject_enabled,
        living_docs_relative_dir: sin.living_docs_relative_dir,
        living_docs_inject_max_chars: sin.living_docs_inject_max_chars,
        living_docs_file_max_each_chars: sin.living_docs_file_max_each_chars,
        project_profile_inject_enabled: sin.project_profile_inject_enabled,
        project_profile_inject_max_chars: sin.project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled: sin.project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars: sin.project_dependency_brief_inject_max_chars,
        tool_call_explain_enabled: sin.tool_call_explain_enabled,
        tool_call_explain_min_chars: sin.tool_call_explain_min_chars,
        tool_call_explain_max_chars: sin.tool_call_explain_max_chars,
        mcp_enabled: sin.mcp_enabled,
        mcp_command: sin.mcp_command,
        mcp_tool_timeout_secs: sin.mcp_tool_timeout_secs,
        web_search_provider: sin.web_search_provider,
        web_search_api_key: sin.web_search_api_key,
        web_search_timeout_secs: sin.web_search_timeout_secs,
        web_search_max_results: sin.web_search_max_results,
        http_fetch_allowed_prefixes: sin.http_fetch_allowed_prefixes,
        http_fetch_timeout_secs: sin.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: sin.http_fetch_max_response_bytes,
    }
}

fn derive_finalize_tail_scalars(mid: &FinalizeAfterRoles) -> Result<FinalizeTailScalars, String> {
    let FinalizeAfterRoles {
        ref b,
        command_timeout_secs,
        max_message_history,
        ref system_prompt_search_bases,
        ref run_command_working_dir,
        ..
    } = *mid;

    let pack = tail_cursor_rules_and_skills_fields(b);
    let ptt = derive_tail_plan_tool_thinking_scalars(
        b,
        system_prompt_search_bases,
        run_command_working_dir.as_path(),
    )?;
    let cqs = derive_tail_context_queues_session_scalars(b, max_message_history);
    let ssw = derive_tail_sandbox_web_scalars(b)?;
    let sin = derive_tail_storage_inject_net_scalars(b, command_timeout_secs)?;
    Ok(assemble_finalize_tail_scalars(pack, ptt, cqs, ssw, sin))
}

