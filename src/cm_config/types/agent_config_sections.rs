//! [`super::AgentConfig`] 的组合式子结构（按运行域分组）。

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use crate::cm_config::FinalPlanRequirementMode;

use super::{
    AgentRoleCatalog, LongTermMemoryScopeMode, LongTermMemoryVectorBackend, PlannerExecutorMode,
    SandboxDockerContainerUser, ScheduledAgentTask, SecretString, SyncDefaultToolSandboxMode,
    WebSearchProvider,
};

/// 会话消息历史相关（上下文裁剪等）；历史文件名 `.crabmate/tui_session.json` 见 `workspace_session`。
#[derive(Debug, Clone)]
pub struct SessionUiConfig {
    pub max_message_history: usize,
}

/// `run_command` 与工作目录。
#[derive(Debug, Clone)]
pub struct CommandExecConfig {
    pub command_timeout_secs: u64,
    pub command_max_output_len: usize,
    pub allowed_commands: Arc<[String]>,
    pub run_command_working_dir: String,
    /// 为 `true` 时，argv 含工作区外绝对路径 / `..` 可经人工审批后执行；默认 `true`。
    pub allow_external_path_with_approval: bool,
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
    /// 允许的 URL 前缀；条目 `"*"` 表示任意 http/https（嵌入默认）。
    pub http_fetch_allowed_prefixes: Vec<String>,
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
    /// `http_fetch` / `http_request` 请求 `User-Agent`（默认 `crabmate/<版本>`；可设为 curl/浏览器 UA 以应对反爬站点）。
    pub http_fetch_user_agent: String,
}

#[derive(Debug, Clone)]
pub struct PerPlanPolicyConfig {
    pub reflection_default_max_rounds: usize,
    pub final_plan_requirement: FinalPlanRequirementMode,
    pub plan_rewrite_max_attempts: usize,
    pub final_plan_require_strict_workflow_node_coverage: bool,
    pub final_plan_semantic_check_enabled: bool,
    /// 是否接受侧向模型旧式单行 `CONSISTENT`/`INCONSISTENT`（默认 `false`，仅 JSON）。
    pub final_plan_semantic_check_accept_legacy_text: bool,
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
    pub final_plan_semantic_check_max_tokens: u32,
    pub planner_executor_mode: PlannerExecutorMode,
    /// 编排档位（当前始终为 `ReAct`，保留用于展示）。
    pub orchestration_profile: crate::cm_config::OrchestrationProfile,
}

#[derive(Debug, Clone)]
pub struct RolesPromptsConfig {
    pub system_prompt: String,
    pub default_agent_role_id: Option<String>,
    pub agent_roles: AgentRoleCatalog,
    /// 是否为默认全局会话与角色叠加编程工作台层（`coding_workbench_increment`）。
    pub coding_workbench_enabled: bool,
    /// 编程层 Markdown 路径（与 `system_prompt_file` 相同解析规则）；`finalize` 读盘。
    pub coding_workbench_increment_file: String,
    /// 未指定请求/会话 `session_mode` 时的默认工作模式（ask / plan / act）。
    pub default_session_mode: crate::cm_types::SessionMode,
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
    /// 工作区层（相对路径相对工作区根；默认同 `.crabmate/skills`）。
    pub skills_dir: String,
    /// 用户级层（跨工作区）。空串表示关闭；默认约定路径见 finalize。
    pub skills_user_dir: String,
    /// 系统级层（如 `/etc/crabmate/skills`）。空串表示关闭；默认约定路径见 finalize。
    pub skills_system_dir: String,
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
    /// 已解析的上下文 LLM 摘要 system（读盘 + 嵌入回退）。
    pub context_summary_system: String,
    /// 已解析的摘要 user 模板；占位符 `{max_tokens}`（或别名 `{max_chars}`）与 `{transcript}`。
    pub context_summary_user_template: String,
}

#[derive(Debug, Clone)]
pub struct WorkspaceRootsConfig {
    pub workspace_allowed_roots: Vec<PathBuf>,
    /// 非空时启用 Web「项目池」：浏览器可用项目名切换/新建子目录，无需手输绝对路径。
    /// 配置时 finalize 要求非空 `workspace_allowed_roots`，且池根落在白名单内、非敏感前缀。
    pub web_workspace_pool: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct WebApiConfig {
    pub web_api_bearer_token: SecretString,
    pub web_api_require_bearer: bool,
    /// 非空时 `serve` 挂载 CORS 白名单（精确 Origin）；空则不挂层。
    /// 未配置时 finalize 注入官方壳默认 Origin；显式空列表可关闭。启动时装配，改后须重启。
    pub web_cors_allowed_origins: Vec<String>,
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
    /// 回合结束后若检测到构建/验证类工具「先失败后成功」，自动写入提炼经验。
    pub long_term_memory_auto_summarize_experience: bool,
    /// 注入前召回时优先 `summarize_experience` / 自动沉淀等经验条，并做标签与关键词加分。
    pub long_term_memory_prioritize_experience_recall: bool,
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
    /// 后台工具任务总开关（`run_command` 的 `async=true`）；默认 `false`。
    pub tool_registry_background_jobs_enabled: bool,
    /// 后台任务同时运行上限；超出进入 `queued`（FIFO）。默认 `4`。
    pub tool_registry_background_job_max_concurrent: u64,
    /// 后台任务排队上限；超限拒绝创建。默认 `32`。
    pub tool_registry_background_job_max_queued: u64,
    /// 后台任务自**创建**起算的保留时长（秒）。默认 `86400`。
    pub tool_registry_background_job_ttl_secs: u64,
    /// 后台任务终态后再保留的宽限（秒），避免"刚完成即被清"。默认 `300`。
    pub tool_registry_background_job_result_grace_secs: u64,
    /// 后台任务注册表条目上限；**仅淘汰终态**条目。默认 `128`。
    pub tool_registry_background_job_max_entries: u64,
}

#[derive(Debug, Clone)]
pub struct TurnBudgetConfig {
    pub max_turn_duration_seconds: u64,
    /// 单轮累计 Token 粗估上限（tiktoken 对齐）；`0` 表示不限制。
    pub max_turn_tokens: usize,
    /// 单轮 LLM 调用次数上限；`0` 表示使用编排层默认（当前 500）。
    pub max_llm_calls_per_turn: u32,
    /// 单 Agent 外循环迭代上限；`0` 表示使用编排层默认（当前 500）。
    pub max_outer_loop_iterations: u32,
    /// 预算接近上限时启用降级策略（跳过分层非关键验收与 Manager 反思 LLM）；默认关闭。
    pub budget_degradation_enabled: bool,
    /// 触发降级的使用比例阈值（50–99）；与 LLM 次数 / Token 粗估取较高者。
    pub budget_degradation_threshold_percent: u8,
}
