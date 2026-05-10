//! [`super::AgentConfig`] 的组合式子结构（按运行域分组）。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::agent::per_coord::FinalPlanRequirementMode;

use super::{
    AgentRoleCatalog, LlmHttpAuthMode, LongTermMemoryScopeMode, LongTermMemoryVectorBackend,
    PlannerExecutorMode, SandboxDockerContainerUser, ScheduledAgentTask, SecretString,
    StagedPlanBaselineMode, StagedPlanFeedbackMode, SyncDefaultToolSandboxMode, WebSearchProvider,
};

/// LLM 网关连接与认证。
#[derive(Debug, Clone)]
pub struct LlmConnectionConfig {
    pub api_base: String,
    pub model: String,
    pub planner_model: Option<String>,
    pub executor_model: Option<String>,
    pub llm_http_auth_mode: LlmHttpAuthMode,
}

/// REPL/TUI 与会话列表相关。
#[derive(Debug, Clone)]
pub struct SessionUiConfig {
    pub max_message_history: usize,
    pub tui_load_session_on_start: bool,
    pub tui_session_max_messages: usize,
    pub repl_initial_workspace_messages_enabled: bool,
}

/// `run_command` 与工作目录。
#[derive(Debug, Clone)]
pub struct CommandExecConfig {
    pub command_timeout_secs: u64,
    pub command_max_output_len: usize,
    pub allowed_commands: Arc<[String]>,
    pub run_command_working_dir: String,
}

/// 采样与上下文窗口计量。
#[derive(Debug, Clone)]
pub struct LlmSamplingConfig {
    pub max_tokens: u32,
    pub llm_context_tokens: u32,
    pub temperature: f32,
    pub llm_seed: Option<i64>,
}

/// 供应商专属请求开关。
#[derive(Debug, Clone)]
pub struct LlmVendorFlagsConfig {
    pub llm_reasoning_split: bool,
    pub llm_bigmodel_thinking: bool,
    pub llm_kimi_thinking_disabled: bool,
}

/// `chat/completions` HTTP 客户端退避。
#[derive(Debug, Clone)]
pub struct LlmHttpRetryConfig {
    pub api_timeout_secs: u64,
    pub api_max_retries: u32,
    pub api_retry_delay_secs: u64,
}

#[derive(Debug, Clone)]
pub struct WeatherToolConfig {
    pub weather_timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct WebSearchConfigSection {
    pub web_search_provider: WebSearchProvider,
    pub web_search_api_key: SecretString,
    pub web_search_timeout_secs: u64,
    pub web_search_max_results: u32,
}

#[derive(Debug, Clone)]
pub struct HttpFetchConfigSection {
    pub http_fetch_allowed_prefixes: Vec<String>,
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
}

#[derive(Debug, Clone)]
pub struct PerPlanPolicyConfig {
    pub reflection_default_max_rounds: usize,
    pub final_plan_requirement: FinalPlanRequirementMode,
    pub plan_rewrite_max_attempts: usize,
    pub final_plan_require_strict_workflow_node_coverage: bool,
    pub final_plan_semantic_check_enabled: bool,
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
    pub final_plan_semantic_check_max_tokens: u32,
    pub planner_executor_mode: PlannerExecutorMode,
}

#[derive(Debug, Clone)]
pub struct RolesPromptsConfig {
    pub system_prompt: String,
    pub default_agent_role_id: Option<String>,
    pub agent_roles: AgentRoleCatalog,
}

#[derive(Debug, Clone)]
pub struct CursorRulesConfigSection {
    pub cursor_rules_enabled: bool,
    pub cursor_rules_dir: String,
    pub cursor_rules_include_agents_md: bool,
    pub cursor_rules_max_chars: usize,
}

#[derive(Debug, Clone)]
pub struct SkillsConfigSection {
    pub skills_enabled: bool,
    pub skills_dir: String,
    pub skills_max_chars: usize,
    pub skills_top_k: usize,
}

#[derive(Debug, Clone)]
pub struct ToolTranscriptConfig {
    pub tool_message_max_chars: usize,
    pub tool_result_envelope_v1: bool,
    pub sse_tool_call_include_arguments: bool,
}

#[derive(Debug, Clone)]
pub struct AgentThinkingTraceConfig {
    pub agent_thinking_trace_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AgentToolStatsConfig {
    pub agent_tool_stats_enabled: bool,
    pub agent_tool_stats_window_events: usize,
    pub agent_tool_stats_min_samples: usize,
    pub agent_tool_stats_max_chars: usize,
    pub agent_tool_stats_warn_below_success_ratio: f64,
}

#[derive(Debug, Clone)]
pub struct DsmlMaterializeConfig {
    pub materialize_deepseek_dsml_tool_calls: bool,
}

#[derive(Debug, Clone)]
pub struct ThinkingEchoConfig {
    pub thinking_avoid_echo_system_prompt: bool,
    pub thinking_avoid_echo_appendix: String,
}

#[derive(Debug, Clone)]
pub struct ContextPipelineConfig {
    pub context_char_budget: usize,
    pub context_min_messages_after_system: usize,
    pub context_summary_trigger_chars: usize,
    pub context_summary_tail_messages: usize,
    pub context_summary_max_tokens: u32,
    pub context_summary_transcript_max_chars: usize,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRootsConfig {
    pub workspace_allowed_roots: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WebApiConfig {
    pub web_api_bearer_token: SecretString,
    pub web_api_require_bearer: bool,
    pub web_audit_log_write_tools: bool,
    pub web_audit_trust_x_forwarded_for: bool,
    pub allow_insecure_no_auth_for_non_loopback: bool,
    pub health_llm_models_probe: bool,
    pub health_llm_models_probe_cache_secs: u64,
}

#[derive(Debug, Clone)]
pub struct ChatQueuesCacheConfig {
    pub chat_queue_max_concurrent: usize,
    pub chat_queue_max_pending: usize,
    pub parallel_readonly_tools_max: usize,
    pub read_file_turn_cache_max_entries: usize,
    /// 进程内只读类 **`run_command`** 缓存 TTL（秒）；**`0`** 关闭。
    pub readonly_tool_ttl_cache_secs: u64,
    /// 上述缓存的最大条目数（跨工作区合计）。
    pub readonly_tool_ttl_cache_max_entries: usize,
    pub test_result_cache_enabled: bool,
    pub test_result_cache_max_entries: usize,
}

#[derive(Debug, Clone)]
pub struct SessionWorkspaceChangelistConfig {
    pub session_workspace_changelist_enabled: bool,
    pub session_workspace_changelist_max_chars: usize,
}

#[derive(Debug, Clone)]
pub struct StagedPlanningConfig {
    pub staged_plan_execution: bool,
    pub staged_plan_phase_instruction: String,
    pub staged_plan_allow_no_task: bool,
    pub staged_plan_feedback_mode: StagedPlanFeedbackMode,
    pub staged_plan_patch_max_attempts: usize,
    pub staged_plan_cli_show_planner_stream: bool,
    pub staged_plan_optimizer_round: bool,
    pub staged_plan_optimizer_requires_parallel_tools: bool,
    pub staged_plan_ensemble_count: u8,
    pub staged_plan_skip_ensemble_on_casual_prompt: bool,
    pub staged_plan_two_phase_nl_display: bool,
    /// 为 **`true`** 时，非分层下 `staged_plan_intent_gate` 对命中架构/咨询启发式的 **`Execute`** 任务**绕过分阶段**；**`false`**（默认）时仍走滚动分阶段。
    pub staged_plan_intent_gate_advisory_bypass: bool,
    /// 首轮定稿计划是否作为后续滚动重规划的蓝图锚点（见 [`StagedPlanBaselineMode`]）。
    pub staged_plan_baseline_mode: StagedPlanBaselineMode,
}

#[derive(Debug, Clone)]
pub struct SyncToolSandboxConfig {
    pub sync_default_tool_sandbox_mode: SyncDefaultToolSandboxMode,
    pub sync_default_tool_sandbox_docker_image: String,
    pub sync_default_tool_sandbox_docker_network: String,
    pub sync_default_tool_sandbox_docker_timeout_secs: u64,
    pub sync_default_tool_sandbox_docker_user: SandboxDockerContainerUser,
}

#[derive(Debug, Clone)]
pub struct ConversationPersistenceConfig {
    pub conversation_store_sqlite_path: String,
    pub scheduled_agent_tasks: Vec<ScheduledAgentTask>,
}

#[derive(Debug, Clone)]
pub struct ContextBootstrapInjectConfig {
    pub agent_memory_file_enabled: bool,
    pub agent_memory_file: String,
    pub agent_memory_file_max_chars: usize,
    pub living_docs_inject_enabled: bool,
    pub living_docs_relative_dir: String,
    pub living_docs_inject_max_chars: usize,
    pub living_docs_file_max_each_chars: usize,
    pub project_profile_inject_enabled: bool,
    pub project_profile_inject_max_chars: usize,
    pub project_dependency_brief_inject_enabled: bool,
    pub project_dependency_brief_inject_max_chars: usize,
}

#[derive(Debug, Clone)]
pub struct ToolCallExplainConfig {
    pub tool_call_explain_enabled: bool,
    pub tool_call_explain_min_chars: usize,
    pub tool_call_explain_max_chars: usize,
}

#[derive(Debug, Clone)]
pub struct LongTermMemoryConfig {
    pub long_term_memory_enabled: bool,
    pub long_term_memory_scope_mode: LongTermMemoryScopeMode,
    pub long_term_memory_vector_backend: LongTermMemoryVectorBackend,
    pub long_term_memory_max_entries: usize,
    pub long_term_memory_inject_max_chars: usize,
    pub long_term_memory_store_sqlite_path: String,
    pub long_term_memory_top_k: usize,
    pub long_term_memory_max_chars_per_chunk: usize,
    pub long_term_memory_min_chars_to_index: usize,
    pub long_term_memory_async_index: bool,
    pub long_term_memory_auto_index_turns: bool,
    pub long_term_memory_default_ttl_secs: u64,
}

#[derive(Debug, Clone)]
pub struct McpClientConfig {
    pub mcp_enabled: bool,
    pub mcp_command: String,
    pub mcp_tool_timeout_secs: u64,
}

#[derive(Debug, Clone)]
pub struct CodebaseSemanticConfig {
    pub codebase_semantic_search_enabled: bool,
    pub codebase_semantic_invalidate_on_workspace_change: bool,
    pub codebase_semantic_index_sqlite_path: String,
    pub codebase_semantic_max_file_bytes: usize,
    pub codebase_semantic_chunk_max_chars: usize,
    pub codebase_semantic_top_k: usize,
    pub codebase_semantic_query_max_chunks: usize,
    pub codebase_semantic_rebuild_max_files: usize,
    pub codebase_semantic_rebuild_incremental: bool,
    pub codebase_semantic_hybrid_alpha: f32,
    pub codebase_semantic_fts_top_n: usize,
    pub codebase_semantic_hybrid_semantic_pool: usize,
}

#[derive(Debug, Clone)]
pub struct ToolRegistryPolicyConfig {
    pub tool_registry_http_fetch_wall_timeout_secs: Option<u64>,
    pub tool_registry_http_request_wall_timeout_secs: Option<u64>,
    pub tool_registry_parallel_wall_timeout_secs: Arc<HashMap<String, u64>>,
    pub tool_registry_parallel_sync_denied_tools: Option<Arc<HashSet<String>>>,
    pub tool_registry_parallel_sync_denied_prefixes: Option<Arc<[String]>>,
    pub tool_registry_sync_default_inline_tools: Option<Arc<HashSet<String>>>,
    pub tool_registry_write_effect_tools: Option<Arc<HashSet<String>>>,
    pub tool_registry_sub_agent_patch_write_extra_tools: Option<Arc<HashSet<String>>>,
    pub tool_registry_sub_agent_test_runner_extra_tools: Option<Arc<HashSet<String>>>,
    pub tool_registry_sub_agent_review_readonly_deny_tools: Option<Arc<HashSet<String>>>,
}

#[derive(Debug, Clone)]
pub struct TurnBudgetConfig {
    pub max_turn_duration_seconds: u64,
    pub max_turn_tokens: usize,
    pub full_plan_rewrite_max_attempts: usize,
}

#[derive(Debug, Clone)]
pub struct HierarchyRoutingConfig {
    pub enable_llm_routing: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct IntentRoutingConfig {
    pub intent_mode_bias_enabled: bool,
    pub intent_l2_enabled: bool,
    pub intent_l2_min_confidence: f32,
    pub intent_l2_max_tokens: u32,
    pub intent_execute_low_threshold: f32,
    pub intent_execute_high_threshold: f32,
    pub intent_non_hier_execute_low_threshold: f32,
    pub intent_non_hier_execute_high_threshold: f32,
    pub intent_at_turn_start_enabled: bool,
    pub intent_l0_routing_boost_enabled: bool,
}
