//! 运行配置：API 地址、模型等，从 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 嵌入默认 + 可选覆盖

mod agent_roles;
mod assembly;
pub mod cli;
mod cursor_rules;
mod source;
mod types;
mod validate;
mod workspace_roots;

use crate::agent::per_coord::FinalPlanRequirementMode;
use source::{AgentRoleRow, AgentSection, ToolRegistrySection, parse_bool_like};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
pub use types::{
    AgentConfig, ExposeSecret, LlmHttpAuthMode, LongTermMemoryScopeMode,
    LongTermMemoryVectorBackend, PlannerExecutorMode, StagedPlanFeedbackMode,
    SyncDefaultToolSandboxMode, WebSearchProvider,
};

/// 进程内共享的 [`AgentConfig`]（`serve` / `repl` / `chat` / `bench`）；热重载时 `write` 更新，回合开始时 `read`+`clone` 得快照传入 `run_agent_turn`。
pub type SharedAgentConfig = std::sync::Arc<tokio::sync::RwLock<AgentConfig>>;

/// 配置累加器：依次接受嵌入默认 TOML → 用户配置文件 → 环境变量的覆盖，最终 `finalize` 为 `AgentConfig`。
#[derive(Default)]
struct ConfigBuilder {
    api_base: String,
    model: String,
    llm_http_auth_mode_str: Option<String>,
    system_prompt: String,
    system_prompt_file: Option<String>,
    max_message_history: Option<u64>,
    tui_load_session_on_start: Option<bool>,
    tui_session_max_messages: Option<u64>,
    repl_initial_workspace_messages_enabled: Option<bool>,
    command_timeout_secs: Option<u64>,
    command_max_output_len: Option<u64>,
    allowed_commands: Option<Vec<String>>,
    run_command_working_dir: Option<String>,
    max_tokens: Option<u64>,
    temperature: Option<f64>,
    llm_seed: Option<i64>,
    llm_reasoning_split: Option<bool>,
    llm_bigmodel_thinking: Option<bool>,
    llm_kimi_thinking_disabled: Option<bool>,
    llm_fold_system_into_user: Option<bool>,
    api_timeout_secs: Option<u64>,
    api_max_retries: Option<u64>,
    api_retry_delay_secs: Option<u64>,
    weather_timeout_secs: Option<u64>,
    web_search_provider_str: Option<String>,
    web_search_api_key: Option<String>,
    web_search_timeout_secs: Option<u64>,
    web_search_max_results: Option<u64>,
    http_fetch_allowed_prefixes: Option<Vec<String>>,
    http_fetch_timeout_secs: Option<u64>,
    http_fetch_max_response_bytes: Option<u64>,
    reflection_default_max_rounds: Option<u64>,
    final_plan_requirement_str: Option<String>,
    plan_rewrite_max_attempts: Option<u64>,
    planner_executor_mode_str: Option<String>,
    cursor_rules_enabled: Option<bool>,
    cursor_rules_dir: Option<String>,
    cursor_rules_include_agents_md: Option<bool>,
    cursor_rules_max_chars: Option<u64>,
    tool_message_max_chars: Option<u64>,
    tool_result_envelope_v1: Option<bool>,
    materialize_deepseek_dsml_tool_calls: Option<bool>,
    context_char_budget: Option<u64>,
    context_min_messages_after_system: Option<u64>,
    context_summary_trigger_chars: Option<u64>,
    context_summary_tail_messages: Option<u64>,
    context_summary_max_tokens: Option<u64>,
    context_summary_transcript_max_chars: Option<u64>,
    health_llm_models_probe: Option<bool>,
    health_llm_models_probe_cache_secs: Option<u64>,
    chat_queue_max_concurrent: Option<u64>,
    chat_queue_max_pending: Option<u64>,
    parallel_readonly_tools_max: Option<u64>,
    read_file_turn_cache_max_entries: Option<u64>,
    test_result_cache_enabled: Option<bool>,
    test_result_cache_max_entries: Option<u64>,
    session_workspace_changelist_enabled: Option<bool>,
    session_workspace_changelist_max_chars: Option<u64>,
    staged_plan_execution: Option<bool>,
    staged_plan_phase_instruction: Option<String>,
    staged_plan_allow_no_task: Option<bool>,
    staged_plan_feedback_mode_str: Option<String>,
    staged_plan_patch_max_attempts: Option<u64>,
    staged_plan_cli_show_planner_stream: Option<bool>,
    staged_plan_optimizer_round: Option<bool>,
    staged_plan_ensemble_count: Option<u64>,
    sync_default_tool_sandbox_mode_str: Option<String>,
    sync_default_tool_sandbox_docker_image: Option<String>,
    sync_default_tool_sandbox_docker_network: Option<String>,
    sync_default_tool_sandbox_docker_timeout_secs: Option<u64>,
    sync_default_tool_sandbox_docker_user: Option<String>,
    workspace_allowed_roots: Option<Vec<String>>,
    web_api_bearer_token: Option<String>,
    allow_insecure_no_auth_for_non_loopback: Option<bool>,
    conversation_store_sqlite_path: Option<String>,
    agent_memory_file_enabled: Option<bool>,
    agent_memory_file: Option<String>,
    agent_memory_file_max_chars: Option<u64>,
    project_profile_inject_enabled: Option<bool>,
    project_profile_inject_max_chars: Option<u64>,
    project_dependency_brief_inject_enabled: Option<bool>,
    project_dependency_brief_inject_max_chars: Option<u64>,
    tool_call_explain_enabled: Option<bool>,
    tool_call_explain_min_chars: Option<u64>,
    tool_call_explain_max_chars: Option<u64>,
    long_term_memory_enabled: Option<bool>,
    long_term_memory_scope_mode_str: Option<String>,
    long_term_memory_vector_backend_str: Option<String>,
    long_term_memory_max_entries: Option<u64>,
    long_term_memory_inject_max_chars: Option<u64>,
    long_term_memory_store_sqlite_path: Option<String>,
    long_term_memory_top_k: Option<u64>,
    long_term_memory_max_chars_per_chunk: Option<u64>,
    long_term_memory_min_chars_to_index: Option<u64>,
    long_term_memory_async_index: Option<bool>,
    mcp_enabled: Option<bool>,
    mcp_command: Option<String>,
    mcp_tool_timeout_secs: Option<u64>,
    codebase_semantic_search_enabled: Option<bool>,
    codebase_semantic_invalidate_on_workspace_change: Option<bool>,
    codebase_semantic_index_sqlite_path: Option<String>,
    codebase_semantic_max_file_bytes: Option<u64>,
    codebase_semantic_chunk_max_chars: Option<u64>,
    codebase_semantic_top_k: Option<u64>,
    codebase_semantic_query_max_chunks: Option<u64>,
    codebase_semantic_rebuild_max_files: Option<u64>,
    /// 见 `[tool_registry]`：`http_fetch` spawn 外圈超时秒数
    tool_registry_http_fetch_wall_timeout_secs: Option<u64>,
    tool_registry_http_request_wall_timeout_secs: Option<u64>,
    tool_registry_parallel_wall_timeout_secs: HashMap<String, u64>,
    tool_registry_parallel_sync_denied_tools: Option<Vec<String>>,
    tool_registry_parallel_sync_denied_prefixes: Option<Vec<String>>,
    tool_registry_sync_default_inline_tools: Option<Vec<String>>,
    tool_registry_write_effect_tools: Option<Vec<String>>,
    /// Web/CLI 未指定 `agent_role` 时使用的默认角色 id（须存在于角色表；与 `agent_roles.toml` / `AGENT_DEFAULT_AGENT_ROLE` 一致）
    default_agent_role_id: Option<String>,
    /// `id -> 未合并条目`；在 [`finalize`] 中与全局 cursor rules 设置一并落成 `AgentConfig.agent_roles`。
    agent_role_entries: HashMap<String, agent_roles::AgentRoleEntryBuilder>,
}

/// 非空 trim 后覆盖 `String` 字段。
fn override_string(dst: &mut String, src: Option<String>) {
    if let Some(s) = src {
        let s = s.trim().to_string();
        if !s.is_empty() {
            *dst = s;
        }
    }
}

/// 非空 trim 后覆盖 `Option<String>` 字段。
fn override_opt_string_non_empty(dst: &mut Option<String>, src: Option<String>) {
    if let Some(s) = src {
        let s = s.trim().to_string();
        if !s.is_empty() {
            *dst = Some(s);
        }
    }
}

/// trim 后覆盖 `Option<String>`（允许空字符串，如 bearer token 可显式清空）。
fn override_opt_string_trimmed(dst: &mut Option<String>, src: Option<&String>) {
    if let Some(s) = src {
        *dst = Some(s.trim().to_string());
    }
}

/// 非空时覆盖 `Option<Vec<String>>`。
fn override_opt_vec(dst: &mut Option<Vec<String>>, src: &Option<Vec<String>>) {
    if let Some(ref v) = *src
        && !v.is_empty()
    {
        *dst = Some(v.clone());
    }
}

impl ConfigBuilder {
    /// 将 `AgentSection` 中有值的字段覆盖到当前累加器。
    fn apply_section(&mut self, agent: AgentSection) {
        override_string(&mut self.api_base, agent.api_base);
        override_string(&mut self.model, agent.model);
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
        self.llm_fold_system_into_user = agent
            .llm_fold_system_into_user
            .or(self.llm_fold_system_into_user);
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
        self.cursor_rules_enabled = agent.cursor_rules_enabled.or(self.cursor_rules_enabled);
        self.cursor_rules_include_agents_md = agent
            .cursor_rules_include_agents_md
            .or(self.cursor_rules_include_agents_md);
        self.cursor_rules_max_chars = agent.cursor_rules_max_chars.or(self.cursor_rules_max_chars);
        self.tool_message_max_chars = agent.tool_message_max_chars.or(self.tool_message_max_chars);
        self.tool_result_envelope_v1 = agent
            .tool_result_envelope_v1
            .or(self.tool_result_envelope_v1);
        self.materialize_deepseek_dsml_tool_calls = agent
            .materialize_deepseek_dsml_tool_calls
            .or(self.materialize_deepseek_dsml_tool_calls);
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
        self.staged_plan_ensemble_count = agent
            .staged_plan_ensemble_count
            .or(self.staged_plan_ensemble_count);
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
    }

    fn merge_agent_role_rows(&mut self, rows: &[AgentRoleRow]) {
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
        }
    }

    fn apply_tool_registry(&mut self, tr: ToolRegistrySection) {
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
    }
}

/// 加载配置：嵌入的 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 为底，再被配置文件覆盖，最后被环境变量覆盖。
/// 若指定 `config_path`，则只从该文件读取覆盖；否则依次尝试 config.toml、.agent_demo.toml。
/// 若最终 api_base、model 或任一运行参数仍未设置则返回错误。
/// 默认 **`system_prompt_file`** 在 [`finalize`] 中按 cwd、各已加载配置文件目录（逆序）、`run_command_working_dir` 解析相对路径。
/// 将 **`load_config` 新结果** 中的「可热更」字段写入 `dst`，保留 **`dst` 中需进程级冻结的项**。
///
/// ## 边界（REPL **`/config reload`** / Web **`POST /config/reload`**）
///
/// - **`API_KEY`**：仍来自**进程环境**；本函数**不**读取或改写密钥，与启动时一致。
/// - **`conversation_store_sqlite_path`**：**不**热更（会话 SQLite 连接在启动时打开；改路径须重启 `serve`）。
/// - **`api_base` / `model` / `llm_http_auth_mode`**：从磁盘+环境变量**重新应用**（与 [`load_config`] 一致），**下一轮** LLM 请求起生效；共享 `reqwest::Client` 的连接池可能短暂保留旧主机空闲连接，直至池超时。
/// - **`health_llm_models_probe` / `health_llm_models_probe_cache_secs`**：热更后下一 **`GET /health`** 起生效；**不**自动清空进程内探测缓存（仍在 TTL 内会继续沿用旧结果直至过期）。
/// - **`system_prompt`**（含 **`system_prompt_file`** 重读）：从 `src` 写入，下一轮起生效。
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
    dst.llm_fold_system_into_user = src.llm_fold_system_into_user;
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
    dst.materialize_deepseek_dsml_tool_calls = src.materialize_deepseek_dsml_tool_calls;
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
    dst.staged_plan_ensemble_count = src.staged_plan_ensemble_count;
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
}

pub fn load_config(config_path: Option<&str>) -> Result<AgentConfig, String> {
    let mut b = ConfigBuilder::default();

    // 嵌入默认分片与用户 TOML 的合并顺序见 `assembly` 模块文档。
    assembly::apply_embedded_config_shards(&mut b)?;
    let system_prompt_search_bases = assembly::merge_user_config_layers(config_path, &mut b)?;

    // 环境变量覆盖（优先级最高）
    apply_env_overrides(&mut b);

    finalize(b, system_prompt_search_bases)
}

/// `system_prompt_file` 相对路径解析：与 `foo.toml` 同目录下的 `config/prompts/...` 等可被找到。
pub(super) fn directory_containing_config_file(config_path: &str) -> PathBuf {
    let p = Path::new(config_path);
    match p.parent() {
        None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        Some(parent) if parent.as_os_str().is_empty() => {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        }
        Some(parent) => parent.to_path_buf(),
    }
}

/// 读取 `system_prompt_file`：绝对路径直接读；否则依次尝试 cwd、各配置目录（后加载的优先）、`run_command_working_dir`。
fn read_system_prompt_file_resolved(
    raw: &str,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    let raw = raw.trim();
    let path = Path::new(raw);
    if path.is_absolute() {
        return std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取 system_prompt_file \"{}\": {}", path.display(), e));
    }

    let mut tried: Vec<String> = Vec::new();

    if let Ok(s) = std::fs::read_to_string(path) {
        return Ok(s);
    }
    tried.push(
        std::env::current_dir()
            .map(|cwd| cwd.join(path).display().to_string())
            .unwrap_or_else(|_| path.display().to_string()),
    );

    for base in config_bases.iter().rev() {
        let candidate = base.join(path);
        if let Ok(s) = std::fs::read_to_string(&candidate) {
            return Ok(s);
        }
        tried.push(candidate.display().to_string());
    }

    let work_candidate = run_command_working_dir.join(path);
    if let Ok(s) = std::fs::read_to_string(&work_candidate) {
        return Ok(s);
    }
    tried.push(work_candidate.display().to_string());

    Err(format!(
        "无法读取 system_prompt_file \"{}\"（相对路径）。已尝试: {}",
        raw,
        tried.join(" | ")
    ))
}

/// 从 `AGENT_*` 环境变量覆盖 `ConfigBuilder` 字段。
fn apply_env_overrides(b: &mut ConfigBuilder) {
    if let Ok(a) = std::env::var("AGENT_API_BASE") {
        let a = a.trim().to_string();
        if !a.is_empty() {
            b.api_base = a;
        }
    }
    if let Ok(m) = std::env::var("AGENT_MODEL") {
        let m = m.trim().to_string();
        if !m.is_empty() {
            b.model = m;
        }
    }
    if let Ok(s) = std::env::var("AGENT_LLM_HTTP_AUTH_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.llm_http_auth_mode_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_MAX_MESSAGE_HISTORY")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.max_message_history = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TUI_SESSION_MAX_MESSAGES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tui_session_max_messages = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TUI_LOAD_SESSION_ON_START")
        && let Some(val) = parse_bool_like(&v)
    {
        b.tui_load_session_on_start = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_REPL_INITIAL_WORKSPACE_MESSAGES_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.repl_initial_workspace_messages_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_COMMAND_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.command_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_COMMAND_MAX_OUTPUT_LEN")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.command_max_output_len = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_ALLOWED_COMMANDS") {
        let list = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !list.is_empty() {
            b.allowed_commands = Some(list);
        }
    }
    if let Ok(v) = std::env::var("AGENT_RUN_COMMAND_WORKING_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.run_command_working_dir = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_WORKSPACE_ALLOWED_ROOTS") {
        let list: Vec<String> = s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !list.is_empty() {
            b.workspace_allowed_roots = Some(list);
        }
    }
    if let Ok(v) = std::env::var("AGENT_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TEMPERATURE")
        && let Ok(n) = v.trim().parse::<f64>()
    {
        b.temperature = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LLM_SEED")
        && let Ok(n) = v.trim().parse::<i64>()
    {
        b.llm_seed = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LLM_REASONING_SPLIT")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_reasoning_split = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_LLM_BIGMODEL_THINKING")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_bigmodel_thinking = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_LLM_KIMI_THINKING_DISABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_kimi_thinking_disabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_LLM_FOLD_SYSTEM_INTO_USER")
        && let Some(val) = parse_bool_like(&v)
    {
        b.llm_fold_system_into_user = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_API_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.api_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_API_MAX_RETRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.api_max_retries = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_API_RETRY_DELAY_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.api_retry_delay_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_WEATHER_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.weather_timeout_secs = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_WEB_SEARCH_PROVIDER") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.web_search_provider_str = Some(s);
        }
    }
    if let Ok(k) = std::env::var("AGENT_WEB_SEARCH_API_KEY") {
        b.web_search_api_key = Some(k);
    }
    if let Ok(v) = std::env::var("AGENT_WEB_SEARCH_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.web_search_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_WEB_SEARCH_MAX_RESULTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.web_search_max_results = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_HTTP_FETCH_ALLOWED_PREFIXES") {
        let list: Vec<String> = s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !list.is_empty() {
            b.http_fetch_allowed_prefixes = Some(list);
        }
    }
    if let Ok(v) = std::env::var("AGENT_HTTP_FETCH_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.http_fetch_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.http_fetch_max_response_bytes = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_REFLECTION_DEFAULT_MAX_ROUNDS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.reflection_default_max_rounds = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_FINAL_PLAN_REQUIREMENT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.final_plan_requirement_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_PLAN_REWRITE_MAX_ATTEMPTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.plan_rewrite_max_attempts = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_PLANNER_EXECUTOR_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.planner_executor_mode_str = Some(s);
        }
    }
    if let Ok(s) = std::env::var("AGENT_SYSTEM_PROMPT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.system_prompt = s;
            b.system_prompt_file = None;
        }
    }
    if let Ok(p) = std::env::var("AGENT_SYSTEM_PROMPT_FILE") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            b.system_prompt_file = Some(p);
        }
    }
    if let Ok(s) = std::env::var("AGENT_DEFAULT_AGENT_ROLE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.default_agent_role_id = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.cursor_rules_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.cursor_rules_dir = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD")
        && let Some(val) = parse_bool_like(&v)
    {
        b.cursor_rules_include_agents_md = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.cursor_rules_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TOOL_MESSAGE_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tool_message_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TOOL_RESULT_ENVELOPE_V1")
        && let Some(val) = parse_bool_like(&v)
    {
        b.tool_result_envelope_v1 = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_MATERIALIZE_DEEPSEEK_DSML_TOOL_CALLS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.materialize_deepseek_dsml_tool_calls = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_CHAR_BUDGET")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_char_budget = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_min_messages_after_system = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_summary_trigger_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_summary_tail_messages = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_summary_max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_summary_transcript_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_HEALTH_LLM_MODELS_PROBE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.health_llm_models_probe = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_HEALTH_LLM_MODELS_PROBE_CACHE_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.health_llm_models_probe_cache_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CHAT_QUEUE_MAX_CONCURRENT")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queue_max_concurrent = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CHAT_QUEUE_MAX_PENDING")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queue_max_pending = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_PARALLEL_READONLY_TOOLS_MAX")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.parallel_readonly_tools_max = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_READ_FILE_TURN_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.read_file_turn_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TEST_RESULT_CACHE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.test_result_cache_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_TEST_RESULT_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.test_result_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_SESSION_WORKSPACE_CHANGELIST_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.session_workspace_changelist_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.session_workspace_changelist_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_EXECUTION")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_plan_execution = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_ALLOW_NO_TASK")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_plan_allow_no_task = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_PHASE_INSTRUCTION") {
        b.staged_plan_phase_instruction = Some(v);
    }
    if let Ok(s) = std::env::var("AGENT_STAGED_PLAN_FEEDBACK_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.staged_plan_feedback_mode_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_PATCH_MAX_ATTEMPTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.staged_plan_patch_max_attempts = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_plan_cli_show_planner_stream = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_OPTIMIZER_ROUND")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_plan_optimizer_round = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_ENSEMBLE_COUNT")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.staged_plan_ensemble_count = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.sync_default_tool_sandbox_mode_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.sync_default_tool_sandbox_docker_image = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK") {
        b.sync_default_tool_sandbox_docker_network = Some(v);
    }
    if let Ok(v) = std::env::var("AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.sync_default_tool_sandbox_docker_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.sync_default_tool_sandbox_docker_user = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_WEB_API_BEARER_TOKEN") {
        b.web_api_bearer_token = Some(v.trim().to_string());
    }
    if let Ok(v) = std::env::var("AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK")
        && let Some(val) = parse_bool_like(&v)
    {
        b.allow_insecure_no_auth_for_non_loopback = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_CONVERSATION_STORE_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.conversation_store_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_MEMORY_FILE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.agent_memory_file_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_MEMORY_FILE") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.agent_memory_file = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_MEMORY_FILE_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.agent_memory_file_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_PROJECT_PROFILE_INJECT_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.project_profile_inject_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_PROJECT_PROFILE_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.project_profile_inject_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.project_dependency_brief_inject_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.project_dependency_brief_inject_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TOOL_CALL_EXPLAIN_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.tool_call_explain_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_TOOL_CALL_EXPLAIN_MIN_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tool_call_explain_min_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TOOL_CALL_EXPLAIN_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.tool_call_explain_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.long_term_memory_enabled = Some(val);
    }
    if let Ok(s) = std::env::var("AGENT_LONG_TERM_MEMORY_SCOPE_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.long_term_memory_scope_mode_str = Some(s);
        }
    }
    if let Ok(s) = std::env::var("AGENT_LONG_TERM_MEMORY_VECTOR_BACKEND") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            b.long_term_memory_vector_backend_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_INJECT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory_inject_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_STORE_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.long_term_memory_store_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_TOP_K")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory_top_k = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory_max_chars_per_chunk = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.long_term_memory_min_chars_to_index = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_LONG_TERM_MEMORY_ASYNC_INDEX")
        && let Some(val) = parse_bool_like(&v)
    {
        b.long_term_memory_async_index = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_MCP_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.mcp_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_MCP_COMMAND") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.mcp_command = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_MCP_TOOL_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.mcp_tool_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_SEARCH_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic_search_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_INVALIDATE_ON_WORKSPACE_CHANGE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic_invalidate_on_workspace_change = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.codebase_semantic_index_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_MAX_FILE_BYTES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic_max_file_bytes = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic_chunk_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_TOP_K")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic_top_k = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_QUERY_MAX_CHUNKS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic_query_max_chunks = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CODEBASE_SEMANTIC_REBUILD_MAX_FILES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic_rebuild_max_files = Some(n);
    }
}

/// `context_char_budget > 0` 且 `context_min_messages_after_system >= max_message_history` 时，按字符删旧消息往往难以生效（条数裁剪已收紧窗口）。
fn context_budget_vs_history_suspicious(
    max_message_history: usize,
    context_char_budget: usize,
    context_min_messages_after_system: usize,
) -> bool {
    context_char_budget > 0 && context_min_messages_after_system >= max_message_history
}

/// 验证、clamp 并组装最终 `AgentConfig`。
fn finalize(
    b: ConfigBuilder,
    system_prompt_search_bases: Vec<PathBuf>,
) -> Result<AgentConfig, String> {
    validate::validate_builder_numeric_ranges(&b)?;
    if b.api_base.is_empty() {
        return Err("配置错误：未设置 api_base（请在 config/default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_BASE 中设置）".to_string());
    }
    if b.model.is_empty() {
        return Err("配置错误：未设置 model（请在 config/default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_MODEL 中设置）".to_string());
    }
    let max_message_history = b.max_message_history.unwrap_or(32).clamp(1, 1024) as usize;
    let tui_load_session_on_start = b.tui_load_session_on_start.unwrap_or(false);
    let tui_session_max_messages =
        b.tui_session_max_messages.unwrap_or(400).clamp(2, 50_000) as usize;
    let repl_initial_workspace_messages_enabled =
        b.repl_initial_workspace_messages_enabled.unwrap_or(false);
    let command_timeout_secs = b.command_timeout_secs.unwrap_or(30).max(1);
    let command_max_output_len =
        b.command_max_output_len.unwrap_or(8192).clamp(1024, 131072) as usize;
    let max_tokens = b.max_tokens.unwrap_or(4096).clamp(256, 32768) as u32;
    let temperature = b.temperature.unwrap_or(0.3).clamp(0.0, 2.0) as f32;
    let api_timeout_secs = b.api_timeout_secs.unwrap_or(60).max(1);
    let api_max_retries = b.api_max_retries.unwrap_or(2).min(10) as u32;
    let api_retry_delay_secs = b.api_retry_delay_secs.unwrap_or(2).max(1);
    let weather_timeout_secs = b.weather_timeout_secs.unwrap_or(15).max(1);
    let reflection_default_max_rounds =
        b.reflection_default_max_rounds.unwrap_or(5).max(1) as usize;

    let allowed_commands_vec = b.allowed_commands.unwrap_or_else(|| {
        vec![
            "aclocal".into(),
            "ar".into(),
            "autoconf".into(),
            "automake".into(),
            "autoreconf".into(),
            "basename".into(),
            "bzcat".into(),
            "c++filt".into(),
            "cargo".into(),
            "cat".into(),
            "clang".into(),
            "clang++".into(),
            "cmake".into(),
            "cmp".into(),
            "column".into(),
            "cut".into(),
            "date".into(),
            "df".into(),
            "diff".into(),
            "dirname".into(),
            "du".into(),
            "echo".into(),
            "egrep".into(),
            "env".into(),
            "expand".into(),
            "fgrep".into(),
            "file".into(),
            "find".into(),
            "fmt".into(),
            "fold".into(),
            "free".into(),
            "g++".into(),
            "gcc".into(),
            "git".into(),
            "grep".into(),
            "head".into(),
            "hexdump".into(),
            "hostname".into(),
            "id".into(),
            "join".into(),
            "jq".into(),
            "ld".into(),
            "ldd".into(),
            "ls".into(),
            "lsblk".into(),
            "lscpu".into(),
            "make".into(),
            "ninja".into(),
            "nl".into(),
            "nm".into(),
            "nproc".into(),
            "objdump".into(),
            "od".into(),
            "paste".into(),
            "pkg-config".into(),
            "printenv".into(),
            "ps".into(),
            "pwd".into(),
            "readelf".into(),
            "readlink".into(),
            "realpath".into(),
            "rev".into(),
            "rustc".into(),
            "seq".into(),
            "size".into(),
            "sort".into(),
            "stat".into(),
            "strings".into(),
            "tac".into(),
            "tail".into(),
            "tr".into(),
            "tree".into(),
            "uname".into(),
            "unexpand".into(),
            "uniq".into(),
            "uptime".into(),
            "wc".into(),
            "whereis".into(),
            "which".into(),
            "whoami".into(),
            "xxd".into(),
            "xzcat".into(),
            "zcat".into(),
        ]
    });
    let allowed_commands: std::sync::Arc<[String]> = allowed_commands_vec.into();

    let run_command_working_dir = b
        .run_command_working_dir
        .ok_or("配置错误：未设置 run_command_working_dir（请在 config/tools.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_RUN_COMMAND_WORKING_DIR 中设置）")?;
    let run_command_working_dir = std::path::Path::new(&run_command_working_dir);
    let run_command_working_dir = match run_command_working_dir.canonicalize() {
        Ok(p) => p,
        Err(e) => {
            return Err(format!(
                "配置错误：run_command_working_dir \"{}\" 不存在或无法解析: {}",
                run_command_working_dir.display(),
                e
            ));
        }
    };
    if !run_command_working_dir.is_dir() {
        return Err(format!(
            "配置错误：run_command_working_dir \"{}\" 不是目录",
            run_command_working_dir.display()
        ));
    }

    let workspace_allowed_roots = workspace_roots::resolve_workspace_allowed_roots(
        b.workspace_allowed_roots,
        run_command_working_dir.as_path(),
    )?;

    let system_prompt = if let Some(ref path) = b.system_prompt_file {
        read_system_prompt_file_resolved(
            path,
            &system_prompt_search_bases,
            run_command_working_dir.as_path(),
        )?
    } else if !b.system_prompt.trim().is_empty() {
        b.system_prompt
    } else {
        return Err(
            "配置错误：未设置 system_prompt_file 或内联 system_prompt（请在 config/default_config.toml、config.toml、环境变量 AGENT_SYSTEM_PROMPT / AGENT_SYSTEM_PROMPT_FILE 中配置）".to_string(),
        );
    };
    if system_prompt.trim().is_empty() {
        return Err("配置错误：system_prompt 从文件或内联加载后为空".to_string());
    }
    let cursor_rules_enabled = b.cursor_rules_enabled.unwrap_or(false);
    let cursor_rules_dir = b
        .cursor_rules_dir
        .unwrap_or_else(|| ".cursor/rules".to_string());
    let cursor_rules_include_agents_md = b.cursor_rules_include_agents_md.unwrap_or(true);
    let cursor_rules_max_chars = b
        .cursor_rules_max_chars
        .unwrap_or(48_000)
        .clamp(1024, 1_000_000);
    let system_prompt = cursor_rules::merge_system_prompt_with_cursor_rules(
        system_prompt,
        cursor_rules_enabled,
        &cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars as usize,
    )?;

    let default_agent_role_id = b
        .default_agent_role_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let (default_agent_role_id, agent_roles) = agent_roles::finalize_agent_role_catalog(
        b.agent_role_entries,
        default_agent_role_id,
        system_prompt.as_str(),
        &system_prompt_search_bases,
        run_command_working_dir.as_path(),
        cursor_rules_enabled,
        &cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars as usize,
    )?;

    let final_plan_requirement = match b.final_plan_requirement_str.as_deref() {
        Some(s) => FinalPlanRequirementMode::parse(s)?,
        None => FinalPlanRequirementMode::default(),
    };
    let plan_rewrite_max_attempts = b.plan_rewrite_max_attempts.unwrap_or(2).clamp(1, 20) as usize;
    let planner_executor_mode = match b.planner_executor_mode_str.as_deref() {
        Some(s) => PlannerExecutorMode::parse(s)?,
        None => PlannerExecutorMode::default(),
    };
    let tool_message_max_chars = b
        .tool_message_max_chars
        .unwrap_or(32768)
        .clamp(1024, 1_048_576) as usize;
    let tool_result_envelope_v1 = b.tool_result_envelope_v1.unwrap_or(true);
    let materialize_deepseek_dsml_tool_calls =
        b.materialize_deepseek_dsml_tool_calls.unwrap_or(true);
    let context_char_budget = b.context_char_budget.unwrap_or(0).min(50_000_000) as usize;
    let context_min_messages_after_system = b
        .context_min_messages_after_system
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
        b.context_summary_trigger_chars.unwrap_or(0).min(50_000_000) as usize;
    let context_summary_tail_messages =
        b.context_summary_tail_messages.unwrap_or(12).clamp(4, 64) as usize;
    let context_summary_max_tokens = b
        .context_summary_max_tokens
        .unwrap_or(1024)
        .clamp(256, 8192) as u32;
    let context_summary_transcript_max_chars = b
        .context_summary_transcript_max_chars
        .unwrap_or(120_000)
        .clamp(10_000, 2_000_000) as usize;
    let health_llm_models_probe = b.health_llm_models_probe.unwrap_or(false);
    let health_llm_models_probe_cache_secs = b
        .health_llm_models_probe_cache_secs
        .unwrap_or(120)
        .clamp(5, 86_400);
    let chat_queue_max_concurrent = b.chat_queue_max_concurrent.unwrap_or(2).clamp(1, 256) as usize;
    let chat_queue_max_pending = b.chat_queue_max_pending.unwrap_or(32).clamp(1, 8192) as usize;
    let parallel_readonly_tools_max = b
        .parallel_readonly_tools_max
        .map(|n| n as usize)
        .unwrap_or_else(|| chat_queue_max_concurrent.max(3))
        .clamp(1, 256);
    let read_file_turn_cache_max_entries =
        b.read_file_turn_cache_max_entries.unwrap_or(64).min(4096) as usize;
    let test_result_cache_enabled = b.test_result_cache_enabled.unwrap_or(true);
    let test_result_cache_max_entries =
        b.test_result_cache_max_entries.unwrap_or(32).clamp(1, 512) as usize;
    let session_workspace_changelist_enabled =
        b.session_workspace_changelist_enabled.unwrap_or(true);
    let session_workspace_changelist_max_chars_raw =
        b.session_workspace_changelist_max_chars.unwrap_or(12_000);
    let session_workspace_changelist_max_chars = if session_workspace_changelist_max_chars_raw == 0
    {
        12_000usize
    } else {
        session_workspace_changelist_max_chars_raw.clamp(2_048, 500_000) as usize
    };
    let staged_plan_execution = b.staged_plan_execution.unwrap_or(true);
    let staged_plan_phase_instruction = b.staged_plan_phase_instruction.unwrap_or_default();
    let staged_plan_allow_no_task = b.staged_plan_allow_no_task.unwrap_or(true);
    let staged_plan_feedback_mode = match b.staged_plan_feedback_mode_str.as_deref() {
        Some(s) => StagedPlanFeedbackMode::parse(s)?,
        None => StagedPlanFeedbackMode::default(),
    };
    let staged_plan_patch_max_attempts =
        b.staged_plan_patch_max_attempts.unwrap_or(2).clamp(1, 16) as usize;
    let staged_plan_cli_show_planner_stream = b.staged_plan_cli_show_planner_stream.unwrap_or(true);
    let staged_plan_optimizer_round = b.staged_plan_optimizer_round.unwrap_or(true);
    let staged_plan_ensemble_count = b.staged_plan_ensemble_count.unwrap_or(1).clamp(1, 3) as u8;
    let sync_default_tool_sandbox_mode = match b.sync_default_tool_sandbox_mode_str.as_deref() {
        Some(s) => types::SyncDefaultToolSandboxMode::parse(s)?,
        None => types::SyncDefaultToolSandboxMode::default(),
    };
    let sync_default_tool_sandbox_docker_image =
        b.sync_default_tool_sandbox_docker_image.unwrap_or_default();
    let sync_default_tool_sandbox_docker_network = b
        .sync_default_tool_sandbox_docker_network
        .unwrap_or_default();
    let sync_default_tool_sandbox_docker_timeout_secs = b
        .sync_default_tool_sandbox_docker_timeout_secs
        .unwrap_or(600)
        .max(1);
    let sync_default_tool_sandbox_docker_user =
        types::SandboxDockerContainerUser::resolve_from_config_str(
            b.sync_default_tool_sandbox_docker_user
                .as_deref()
                .unwrap_or(""),
        );
    if sync_default_tool_sandbox_mode == types::SyncDefaultToolSandboxMode::Docker
        && sync_default_tool_sandbox_docker_image.trim().is_empty()
    {
        return Err(
            "配置错误：sync_default_tool_sandbox_mode=docker 时必须设置非空的 sync_default_tool_sandbox_docker_image"
                .to_string(),
        );
    }
    let web_api_bearer_token =
        types::SecretString::new(b.web_api_bearer_token.unwrap_or_default().into());
    let allow_insecure_no_auth_for_non_loopback =
        b.allow_insecure_no_auth_for_non_loopback.unwrap_or(false);

    let conversation_store_sqlite_path = b.conversation_store_sqlite_path.unwrap_or_default();
    let agent_memory_file_enabled = b.agent_memory_file_enabled.unwrap_or(false);
    let agent_memory_file = b
        .agent_memory_file
        .unwrap_or_else(|| ".crabmate/agent_memory.md".to_string());
    let agent_memory_file_max_chars = b
        .agent_memory_file_max_chars
        .unwrap_or(8000)
        .clamp(256, 500_000) as usize;
    let project_profile_inject_enabled = b.project_profile_inject_enabled.unwrap_or(true);
    let project_profile_inject_max_chars = b
        .project_profile_inject_max_chars
        .unwrap_or(6000)
        .clamp(0, 500_000) as usize;
    let project_dependency_brief_inject_enabled =
        b.project_dependency_brief_inject_enabled.unwrap_or(true);
    let project_dependency_brief_inject_max_chars = b
        .project_dependency_brief_inject_max_chars
        .unwrap_or(4000)
        .clamp(0, 500_000) as usize;
    let tool_call_explain_enabled = b.tool_call_explain_enabled.unwrap_or(false);
    let tool_call_explain_min_chars =
        b.tool_call_explain_min_chars.unwrap_or(8).clamp(1, 256) as usize;
    let max_chars_raw = b.tool_call_explain_max_chars.unwrap_or(400).clamp(1, 4000) as usize;
    let tool_call_explain_max_chars = max_chars_raw.max(tool_call_explain_min_chars);

    let long_term_memory_enabled = b.long_term_memory_enabled.unwrap_or(true);
    let long_term_memory_scope_mode = match b.long_term_memory_scope_mode_str.as_deref() {
        Some(s) => LongTermMemoryScopeMode::parse(s)?,
        None => LongTermMemoryScopeMode::default(),
    };
    let long_term_memory_vector_backend = match b.long_term_memory_vector_backend_str.as_deref() {
        Some(s) => LongTermMemoryVectorBackend::parse(s)?,
        None => LongTermMemoryVectorBackend::default(),
    };
    if long_term_memory_enabled {
        match long_term_memory_vector_backend {
            LongTermMemoryVectorBackend::Qdrant | LongTermMemoryVectorBackend::Pgvector => {
                return Err(
                    "配置错误：长期记忆向量后端 qdrant / pgvector 尚未接入；请使用 disabled 或 fastembed，或关闭 long_term_memory_enabled"
                        .to_string(),
                );
            }
            LongTermMemoryVectorBackend::Disabled | LongTermMemoryVectorBackend::Fastembed => {}
        }
    }
    let long_term_memory_max_entries = b
        .long_term_memory_max_entries
        .unwrap_or(256)
        .clamp(1, 100_000) as usize;
    let long_term_memory_inject_max_chars = b
        .long_term_memory_inject_max_chars
        .unwrap_or(8000)
        .clamp(256, 500_000) as usize;
    let long_term_memory_store_sqlite_path =
        b.long_term_memory_store_sqlite_path.unwrap_or_default();
    let long_term_memory_top_k = b.long_term_memory_top_k.unwrap_or(8).clamp(1, 64) as usize;
    let long_term_memory_max_chars_per_chunk = b
        .long_term_memory_max_chars_per_chunk
        .unwrap_or(1024)
        .clamp(256, 32_000) as usize;
    let long_term_memory_min_chars_to_index = b
        .long_term_memory_min_chars_to_index
        .unwrap_or(8)
        .clamp(0, 4096) as usize;
    let long_term_memory_async_index = b.long_term_memory_async_index.unwrap_or(true);

    let mcp_enabled = b.mcp_enabled.unwrap_or(false);
    let mcp_command = b.mcp_command.unwrap_or_default();
    let mcp_tool_timeout_secs = b
        .mcp_tool_timeout_secs
        .unwrap_or(command_timeout_secs)
        .max(1);

    let codebase_semantic_search_enabled = b.codebase_semantic_search_enabled.unwrap_or(true);
    let codebase_semantic_invalidate_on_workspace_change = b
        .codebase_semantic_invalidate_on_workspace_change
        .unwrap_or(true);
    let codebase_semantic_index_sqlite_path =
        b.codebase_semantic_index_sqlite_path.unwrap_or_default();
    let codebase_semantic_max_file_bytes = b
        .codebase_semantic_max_file_bytes
        .unwrap_or(512 * 1024)
        .clamp(4096, 4 * 1024 * 1024) as usize;
    let codebase_semantic_chunk_max_chars = b
        .codebase_semantic_chunk_max_chars
        .unwrap_or(1200)
        .clamp(256, 16_000) as usize;
    let codebase_semantic_top_k = b.codebase_semantic_top_k.unwrap_or(8).clamp(1, 64) as usize;
    let codebase_semantic_query_max_chunks = b
        .codebase_semantic_query_max_chunks
        .unwrap_or(50_000)
        .clamp(0, 2_000_000) as usize;
    let codebase_semantic_rebuild_max_files = b
        .codebase_semantic_rebuild_max_files
        .unwrap_or(2000)
        .clamp(1, 100_000) as usize;

    let web_search_provider = match b.web_search_provider_str.as_deref() {
        Some(s) => WebSearchProvider::parse(s)?,
        None => WebSearchProvider::default(),
    };
    let web_search_api_key =
        types::SecretString::new(b.web_search_api_key.unwrap_or_default().into());
    let web_search_timeout_secs = b.web_search_timeout_secs.unwrap_or(30).max(1);
    let web_search_max_results = b.web_search_max_results.unwrap_or(8).clamp(1, 20) as u32;

    let http_fetch_allowed_prefixes = b.http_fetch_allowed_prefixes.unwrap_or_default();
    let http_fetch_timeout_secs = b.http_fetch_timeout_secs.unwrap_or(30).max(1);
    let http_fetch_max_response_bytes = b
        .http_fetch_max_response_bytes
        .unwrap_or(524_288)
        .clamp(1024, 4_194_304) as usize;

    let tool_registry_http_fetch_wall_timeout_secs = b
        .tool_registry_http_fetch_wall_timeout_secs
        .map(|s| s.clamp(1, 86_400));
    let tool_registry_http_request_wall_timeout_secs = b
        .tool_registry_http_request_wall_timeout_secs
        .map(|s| s.clamp(1, 86_400));
    let tool_registry_parallel_wall_timeout_secs = Arc::new(
        b.tool_registry_parallel_wall_timeout_secs
            .into_iter()
            .map(|(k, v)| (k, v.clamp(1, 86_400)))
            .collect::<HashMap<_, _>>(),
    );
    let tool_registry_parallel_sync_denied_tools =
        b.tool_registry_parallel_sync_denied_tools.map(|v| {
            Arc::new(
                v.into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>(),
            )
        });
    let tool_registry_parallel_sync_denied_prefixes =
        b.tool_registry_parallel_sync_denied_prefixes.map(|v| {
            let cleaned: Vec<String> = v
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            Arc::from(cleaned.into_boxed_slice())
        });
    let tool_registry_sync_default_inline_tools =
        b.tool_registry_sync_default_inline_tools.map(|v| {
            Arc::new(
                v.into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>(),
            )
        });
    let tool_registry_write_effect_tools = b.tool_registry_write_effect_tools.map(|v| {
        Arc::new(
            v.into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>(),
        )
    });

    let llm_http_auth_mode = match b.llm_http_auth_mode_str.as_deref() {
        Some(s) => types::LlmHttpAuthMode::parse(s)?,
        None => types::LlmHttpAuthMode::default(),
    };

    let llm_reasoning_split = b.llm_reasoning_split.unwrap_or_else(|| {
        crate::llm::vendor::default_llm_reasoning_split_for_gateway(&b.model, &b.api_base)
    });

    Ok(AgentConfig {
        api_base: b.api_base,
        model: b.model,
        llm_http_auth_mode,
        max_message_history,
        tui_load_session_on_start,
        tui_session_max_messages,
        repl_initial_workspace_messages_enabled,
        command_timeout_secs,
        command_max_output_len,
        allowed_commands,
        run_command_working_dir: run_command_working_dir.display().to_string(),
        max_tokens,
        temperature,
        llm_seed: b.llm_seed,
        llm_reasoning_split,
        llm_bigmodel_thinking: b.llm_bigmodel_thinking.unwrap_or(false),
        llm_kimi_thinking_disabled: b.llm_kimi_thinking_disabled.unwrap_or(false),
        llm_fold_system_into_user: b.llm_fold_system_into_user.unwrap_or(false),
        api_timeout_secs,
        api_max_retries,
        api_retry_delay_secs,
        weather_timeout_secs,
        web_search_provider,
        web_search_api_key,
        web_search_timeout_secs,
        web_search_max_results,
        http_fetch_allowed_prefixes,
        http_fetch_timeout_secs,
        http_fetch_max_response_bytes,
        reflection_default_max_rounds,
        final_plan_requirement,
        plan_rewrite_max_attempts,
        planner_executor_mode,
        system_prompt,
        default_agent_role_id,
        agent_roles,
        cursor_rules_enabled,
        cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars: cursor_rules_max_chars as usize,
        tool_message_max_chars,
        tool_result_envelope_v1,
        materialize_deepseek_dsml_tool_calls,
        context_char_budget,
        context_min_messages_after_system,
        context_summary_trigger_chars,
        context_summary_tail_messages,
        context_summary_max_tokens,
        context_summary_transcript_max_chars,
        workspace_allowed_roots,
        web_api_bearer_token,
        allow_insecure_no_auth_for_non_loopback,
        health_llm_models_probe,
        health_llm_models_probe_cache_secs,
        chat_queue_max_concurrent,
        chat_queue_max_pending,
        parallel_readonly_tools_max,
        read_file_turn_cache_max_entries,
        test_result_cache_enabled,
        test_result_cache_max_entries,
        session_workspace_changelist_enabled,
        session_workspace_changelist_max_chars,
        staged_plan_execution,
        staged_plan_phase_instruction,
        staged_plan_allow_no_task,
        staged_plan_feedback_mode,
        staged_plan_patch_max_attempts,
        staged_plan_cli_show_planner_stream,
        staged_plan_optimizer_round,
        staged_plan_ensemble_count,
        sync_default_tool_sandbox_mode,
        sync_default_tool_sandbox_docker_image,
        sync_default_tool_sandbox_docker_network,
        sync_default_tool_sandbox_docker_timeout_secs,
        sync_default_tool_sandbox_docker_user,
        conversation_store_sqlite_path,
        agent_memory_file_enabled,
        agent_memory_file,
        agent_memory_file_max_chars,
        project_profile_inject_enabled,
        project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars,
        tool_call_explain_enabled,
        tool_call_explain_min_chars,
        tool_call_explain_max_chars,
        long_term_memory_enabled,
        long_term_memory_scope_mode,
        long_term_memory_vector_backend,
        long_term_memory_max_entries,
        long_term_memory_inject_max_chars,
        long_term_memory_store_sqlite_path,
        long_term_memory_top_k,
        long_term_memory_max_chars_per_chunk,
        long_term_memory_min_chars_to_index,
        long_term_memory_async_index,
        mcp_enabled,
        mcp_command,
        mcp_tool_timeout_secs,
        codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change,
        codebase_semantic_index_sqlite_path,
        codebase_semantic_max_file_bytes,
        codebase_semantic_chunk_max_chars,
        codebase_semantic_top_k,
        codebase_semantic_query_max_chunks,
        codebase_semantic_rebuild_max_files,
        tool_registry_http_fetch_wall_timeout_secs,
        tool_registry_http_request_wall_timeout_secs,
        tool_registry_parallel_wall_timeout_secs,
        tool_registry_parallel_sync_denied_tools,
        tool_registry_parallel_sync_denied_prefixes,
        tool_registry_sync_default_inline_tools,
        tool_registry_write_effect_tools,
    })
}

#[cfg(test)]
mod embedded_shard_parse_tests {
    use super::ConfigBuilder;
    use super::assembly;

    #[test]
    fn malformed_embedded_toml_returns_err_naming_shard() {
        let mut b = ConfigBuilder::default();
        let err =
            assembly::apply_embedded_agent_shard_for_test(&mut b, "test_shard.toml", "[[[not toml")
                .unwrap_err();
        assert!(
            err.contains("test_shard.toml"),
            "expected shard label in error: {err}"
        );
        assert!(
            err.contains("嵌入默认配置"),
            "expected Chinese prefix in error: {err}"
        );
    }
}

#[cfg(test)]
mod llm_reasoning_split_default_tests {
    use super::load_config;
    use std::fs;

    #[test]
    fn finalize_respects_omitted_reasoning_split_for_non_minimax() {
        assert!(
            !crate::llm::vendor::default_llm_reasoning_split_for_gateway(
                "deepseek-chat",
                "https://api.deepseek.com/v1",
            )
        );
    }

    #[test]
    fn minimax_user_toml_without_key_defaults_true() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent.toml");
        fs::write(
            &path,
            r#"[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"
llm_fold_system_into_user = true
"#,
        )
        .expect("write");
        let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
        assert!(
            cfg.llm_reasoning_split,
            "MiniMax 网关未写 llm_reasoning_split 时应默认 true"
        );
    }

    #[test]
    fn minimax_user_toml_explicit_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent.toml");
        fs::write(
            &path,
            r#"[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"
llm_reasoning_split = false
"#,
        )
        .expect("write");
        let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
        assert!(!cfg.llm_reasoning_split);
    }
}

#[cfg(test)]
mod hot_reload_tests {
    use super::{apply_hot_reload_config_subset, load_config};

    #[test]
    fn apply_hot_reload_keeps_conversation_store_path() {
        let base = load_config(None).expect("default config");
        let mut dst = base.clone();
        let frozen = dst.conversation_store_sqlite_path.clone();
        let mut src = dst.clone();
        src.conversation_store_sqlite_path = "/tmp/should_not_apply.sqlite".to_string();
        apply_hot_reload_config_subset(&mut dst, &src);
        assert_eq!(dst.conversation_store_sqlite_path, frozen);
    }
}

#[cfg(test)]
mod context_budget_warning_tests {
    use super::context_budget_vs_history_suspicious;

    #[test]
    fn suspicious_when_budget_on_and_min_ge_max_history() {
        assert!(context_budget_vs_history_suspicious(8, 100_000, 8));
        assert!(context_budget_vs_history_suspicious(8, 1, 10));
    }

    #[test]
    fn not_suspicious_when_budget_off() {
        assert!(!context_budget_vs_history_suspicious(8, 0, 100));
    }

    #[test]
    fn not_suspicious_when_min_below_max_history() {
        assert!(!context_budget_vs_history_suspicious(32, 50_000, 4));
    }
}

#[cfg(test)]
mod numeric_validate_tests {
    use super::ConfigBuilder;
    use super::validate;
    use std::collections::HashMap;

    #[test]
    fn rejects_temperature_above_two() {
        let b = ConfigBuilder {
            temperature: Some(3.0),
            ..Default::default()
        };
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("temperature"), "err: {err}");
    }

    #[test]
    fn parallel_wall_timeout_out_of_range() {
        let mut m = HashMap::new();
        m.insert("http_fetch_spawn_timeout".into(), 100_000u64);
        let b = ConfigBuilder {
            tool_registry_parallel_wall_timeout_secs: m,
            ..Default::default()
        };
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("parallel_wall_timeout_secs"), "err: {err}");
    }
}
