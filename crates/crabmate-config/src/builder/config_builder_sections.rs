//! [`super::ConfigBuilder`] 的组合式子结构（与 [`crate::config::types::AgentConfig`] 域对齐，仍为 `Option`/原始累加形态）。

use std::collections::HashMap;

use crate::agent_roles;
use crate::source::ScheduledAgentTaskRow;

/// LLM 网关与认证字符串（`finalize` 再解析枚举）。
#[derive(Default)]
pub(crate) struct ConfigBuilderLlm {
    pub(crate) api_base: String,
    pub(crate) model: String,
    pub(crate) planner_model: Option<String>,
    pub(crate) executor_model: Option<String>,
    pub(crate) llm_http_auth_mode_str: Option<String>,
}

/// 系统提示与默认角色 id（角色表条目在 [`super::ConfigBuilder`] 顶层）。
#[derive(Default)]
pub(crate) struct ConfigBuilderRolesPrompts {
    pub(crate) system_prompt: String,
    pub(crate) system_prompt_file: Option<String>,
    pub(crate) default_agent_role_id: Option<String>,
    pub(crate) coding_workbench_enabled: Option<bool>,
    pub(crate) coding_workbench_increment_file: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderSessionUi {
    pub(crate) max_message_history: Option<u64>,
    pub(crate) tui_load_session_on_start: Option<bool>,
    pub(crate) tui_session_max_messages: Option<u64>,
    pub(crate) repl_initial_workspace_messages_enabled: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderCommandExec {
    pub(crate) command_timeout_secs: Option<u64>,
    pub(crate) command_max_output_len: Option<u64>,
    pub(crate) allowed_commands: Option<Vec<String>>,
    pub(crate) run_command_working_dir: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderLlmSampling {
    pub(crate) max_tokens: Option<u64>,
    pub(crate) llm_context_tokens: Option<u64>,
    pub(crate) temperature: Option<f64>,
    pub(crate) llm_seed: Option<i64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderLlmVendor {
    pub(crate) llm_reasoning_split: Option<bool>,
    pub(crate) llm_bigmodel_thinking: Option<bool>,
    pub(crate) llm_kimi_thinking_disabled: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderLlmHttpRetry {
    pub(crate) api_timeout_secs: Option<u64>,
    pub(crate) api_max_retries: Option<u64>,
    pub(crate) api_retry_delay_secs: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderWeatherTool {
    pub(crate) weather_timeout_secs: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderWebSearch {
    pub(crate) web_search_provider_str: Option<String>,
    pub(crate) web_search_api_key: Option<String>,
    pub(crate) web_search_timeout_secs: Option<u64>,
    pub(crate) web_search_max_results: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderHttpFetch {
    pub(crate) http_fetch_allowed_prefixes: Option<Vec<String>>,
    pub(crate) http_fetch_timeout_secs: Option<u64>,
    pub(crate) http_fetch_max_response_bytes: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderPerPlanPolicy {
    pub(crate) reflection_default_max_rounds: Option<u64>,
    pub(crate) final_plan_requirement_str: Option<String>,
    pub(crate) plan_rewrite_max_attempts: Option<u64>,
    pub(crate) final_plan_require_strict_workflow_node_coverage: Option<bool>,
    pub(crate) final_plan_semantic_check_enabled: Option<bool>,
    pub(crate) final_plan_semantic_check_max_non_readonly_tools: Option<u64>,
    pub(crate) final_plan_semantic_check_max_tokens: Option<u64>,
    pub(crate) planner_executor_mode_str: Option<String>,
    pub(crate) orchestration_profile_str: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderCursorRules {
    pub(crate) cursor_rules_enabled: Option<bool>,
    pub(crate) cursor_rules_dir: Option<String>,
    pub(crate) cursor_rules_include_agents_md: Option<bool>,
    pub(crate) cursor_rules_max_chars: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderSkills {
    pub(crate) skills_enabled: Option<bool>,
    pub(crate) skills_dir: Option<String>,
    pub(crate) skills_max_chars: Option<u64>,
    pub(crate) skills_top_k: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderToolTranscript {
    pub(crate) tool_message_max_chars: Option<u64>,
    pub(crate) tool_result_envelope_v1: Option<bool>,
    pub(crate) sse_tool_call_include_arguments: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderAgentThinkingTrace {
    /// 仅环境变量 `CM_THINKING_TRACE_ENABLED` 写入；**不**从 `[agent]` TOML 合并。
    pub(crate) agent_thinking_trace_enabled: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderAgentToolStats {
    pub(crate) agent_tool_stats_enabled: Option<bool>,
    pub(crate) agent_tool_stats_window_events: Option<u64>,
    pub(crate) agent_tool_stats_min_samples: Option<u64>,
    pub(crate) agent_tool_stats_max_chars: Option<u64>,
    pub(crate) agent_tool_stats_warn_below_success_ratio: Option<f64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderDsmlMaterialize {
    pub(crate) materialize_deepseek_dsml_tool_calls: Option<bool>,
    pub(crate) dsml_stream_strip_enabled: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderThinkingEcho {
    pub(crate) thinking_avoid_echo_system_prompt: Option<bool>,
    pub(crate) thinking_avoid_echo_appendix: Option<String>,
    pub(crate) thinking_avoid_echo_appendix_file: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderContextPipeline {
    pub(crate) context_char_budget: Option<u64>,
    pub(crate) context_min_messages_after_system: Option<u64>,
    pub(crate) context_summary_trigger_chars: Option<u64>,
    pub(crate) context_summary_tail_messages: Option<u64>,
    pub(crate) context_summary_max_tokens: Option<u64>,
    pub(crate) context_summary_transcript_max_chars: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderWorkspaceRoots {
    pub(crate) workspace_allowed_roots: Option<Vec<String>>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderWebApi {
    pub(crate) web_api_bearer_token: Option<String>,
    pub(crate) web_api_require_bearer: Option<bool>,
    pub(crate) web_audit_log_write_tools: Option<bool>,
    pub(crate) web_audit_trust_x_forwarded_for: Option<bool>,
    pub(crate) allow_insecure_no_auth_for_non_loopback: Option<bool>,
    pub(crate) health_llm_models_probe: Option<bool>,
    pub(crate) health_llm_models_probe_cache_secs: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderChatQueuesCache {
    pub(crate) chat_queue_max_concurrent: Option<u64>,
    pub(crate) chat_queue_max_pending: Option<u64>,
    pub(crate) parallel_readonly_tools_max: Option<u64>,
    pub(crate) read_file_turn_cache_max_entries: Option<u64>,
    pub(crate) readonly_tool_ttl_cache_secs: Option<u64>,
    pub(crate) readonly_tool_ttl_cache_max_entries: Option<u64>,
    pub(crate) test_result_cache_enabled: Option<bool>,
    pub(crate) test_result_cache_max_entries: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderSessionWorkspaceChangelist {
    pub(crate) session_workspace_changelist_enabled: Option<bool>,
    pub(crate) session_workspace_changelist_max_chars: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderStagedPlanning {
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
    pub(crate) staged_plan_intent_gate_advisory_bypass: Option<bool>,
    pub(crate) staged_plan_advisory_bypass_extra_impl_blockers: Option<Vec<String>>,
    pub(crate) staged_plan_advisory_bypass_extra_arch_markers: Option<Vec<String>>,
    pub(crate) staged_plan_advisory_bypass_extra_consult_markers: Option<Vec<String>>,
    pub(crate) staged_plan_baseline_mode_str: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderSyncToolSandbox {
    pub(crate) sync_default_tool_sandbox_mode_str: Option<String>,
    pub(crate) sync_default_tool_sandbox_docker_image: Option<String>,
    pub(crate) sync_default_tool_sandbox_docker_network: Option<String>,
    pub(crate) sync_default_tool_sandbox_docker_timeout_secs: Option<u64>,
    pub(crate) sync_default_tool_sandbox_docker_user: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderConversationPersistence {
    pub(crate) conversation_store_sqlite_path: Option<String>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderContextBootstrapInject {
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
}

#[derive(Default)]
pub(crate) struct ConfigBuilderToolCallExplain {
    pub(crate) tool_call_explain_enabled: Option<bool>,
    pub(crate) tool_call_explain_min_chars: Option<u64>,
    pub(crate) tool_call_explain_max_chars: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderLongTermMemory {
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
    pub(crate) long_term_memory_auto_summarize_experience: Option<bool>,
    pub(crate) long_term_memory_prioritize_experience_recall: Option<bool>,
    pub(crate) long_term_memory_default_ttl_secs: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderMcpClient {
    pub(crate) mcp_enabled: Option<bool>,
    pub(crate) mcp_command: Option<String>,
    pub(crate) mcp_tool_timeout_secs: Option<u64>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderCodebaseSemantic {
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
}

#[derive(Default)]
pub(crate) struct ConfigBuilderIntentRouting {
    pub(crate) intent_execute_low_threshold: Option<f64>,
    pub(crate) intent_execute_high_threshold: Option<f64>,
    pub(crate) intent_non_hier_execute_low_threshold: Option<f64>,
    pub(crate) intent_non_hier_execute_high_threshold: Option<f64>,
    pub(crate) intent_mode_bias_enabled: Option<bool>,
    pub(crate) intent_l2_enabled: Option<bool>,
    pub(crate) intent_l2_min_confidence: Option<f64>,
    pub(crate) intent_l2_max_tokens: Option<u64>,
    pub(crate) intent_at_turn_start_enabled: Option<bool>,
    pub(crate) intent_l0_routing_boost_enabled: Option<bool>,
}

#[derive(Default)]
pub(crate) struct ConfigBuilderToolRegistryPolicy {
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
}

/// 配置累加器：依次接受嵌入默认 TOML → 用户配置文件 → 环境变量的覆盖，最终 `finalize` 为 `AgentConfig`。
#[derive(Default)]
pub(crate) struct ConfigBuilder {
    pub(crate) llm: ConfigBuilderLlm,
    pub(crate) roles_prompts: ConfigBuilderRolesPrompts,
    pub(crate) session_ui: ConfigBuilderSessionUi,
    pub(crate) command_exec: ConfigBuilderCommandExec,
    pub(crate) llm_sampling: ConfigBuilderLlmSampling,
    pub(crate) llm_vendor: ConfigBuilderLlmVendor,
    pub(crate) llm_http_retry: ConfigBuilderLlmHttpRetry,
    pub(crate) weather_tool: ConfigBuilderWeatherTool,
    pub(crate) web_search: ConfigBuilderWebSearch,
    pub(crate) http_fetch: ConfigBuilderHttpFetch,
    pub(crate) per_plan_policy: ConfigBuilderPerPlanPolicy,
    pub(crate) cursor_rules: ConfigBuilderCursorRules,
    pub(crate) skills: ConfigBuilderSkills,
    pub(crate) tool_transcript: ConfigBuilderToolTranscript,
    pub(crate) agent_thinking_trace: ConfigBuilderAgentThinkingTrace,
    pub(crate) agent_tool_stats: ConfigBuilderAgentToolStats,
    pub(crate) dsml_materialize: ConfigBuilderDsmlMaterialize,
    pub(crate) thinking_echo: ConfigBuilderThinkingEcho,
    pub(crate) context_pipeline: ConfigBuilderContextPipeline,
    pub(crate) workspace_roots: ConfigBuilderWorkspaceRoots,
    pub(crate) web_api: ConfigBuilderWebApi,
    pub(crate) chat_queues_cache: ConfigBuilderChatQueuesCache,
    pub(crate) session_workspace_changelist: ConfigBuilderSessionWorkspaceChangelist,
    pub(crate) staged_planning: ConfigBuilderStagedPlanning,
    pub(crate) sync_tool_sandbox: ConfigBuilderSyncToolSandbox,
    pub(crate) conversation_persistence: ConfigBuilderConversationPersistence,
    pub(crate) context_bootstrap_inject: ConfigBuilderContextBootstrapInject,
    pub(crate) tool_call_explain: ConfigBuilderToolCallExplain,
    pub(crate) long_term_memory: ConfigBuilderLongTermMemory,
    pub(crate) mcp_client: ConfigBuilderMcpClient,
    pub(crate) codebase_semantic: ConfigBuilderCodebaseSemantic,
    pub(crate) intent_routing: ConfigBuilderIntentRouting,
    pub(crate) tool_registry_policy: ConfigBuilderToolRegistryPolicy,
    /// `id -> 未合并条目`；在 [`crate::config::finalize`] 中与全局 cursor rules 设置一并落成 `AgentConfig.roles_prompts.agent_roles`。
    pub(crate) agent_role_entries: HashMap<String, agent_roles::AgentRoleEntryBuilder>,
    /// 顶层 `[[scheduled_agent_task]]` 合并结果（仅用户 `config.toml` 等；嵌入默认分片不含此项）。
    pub(crate) scheduled_agent_task_rows: Vec<ScheduledAgentTaskRow>,
}
