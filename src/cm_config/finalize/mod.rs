use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cm_config::FinalPlanRequirementMode;

use super::agent_roles;
use super::builder::ConfigBuilder;
use super::skills;
use super::types::{
    self, AgentConfig, LongTermMemoryVectorBackend, ScheduledAgentTask, WebSearchProvider,
};
use super::validate;
use super::workspace_roots;
use crate::cm_names as tool_names;

mod agent_layers;
mod checks;
mod derived;
mod prompts;

pub use self::prompts::{
    embedded_context_summary_system, embedded_context_summary_user_template,
    embedded_session_mode_appendix, embedded_thinking_avoid_echo_appendix,
};
pub(crate) use self::prompts::{resolve_coding_workbench_increment, resolve_context_summary_prompts};
mod build;
mod tail;

pub(in crate::cm_config) use self::build::finalize;

#[cfg(test)]
#[cfg(test)]
mod tests;

/// `context_char_budget > 0` 且 `context_min_messages_after_system >= max_message_history` 时，按字符删旧消息往往难以生效（条数裁剪已收紧窗口）。
pub(crate) fn context_budget_vs_history_suspicious(
    max_message_history: usize,
    context_char_budget: usize,
    context_min_messages_after_system: usize,
) -> bool {
    context_char_budget > 0 && context_min_messages_after_system >= max_message_history
}

struct ToolRegistryDerived {
    tool_registry_http_fetch_wall_timeout_secs: Option<u64>,
    tool_registry_http_request_wall_timeout_secs: Option<u64>,
    tool_registry_parallel_wall_timeout_secs: Arc<HashMap<String, u64>>,
    tool_registry_parallel_sync_denied_tools: Option<Arc<HashSet<String>>>,
    tool_registry_parallel_sync_denied_prefixes: Option<Arc<[String]>>,
    tool_registry_sync_default_inline_tools: Option<Arc<HashSet<String>>>,
    tool_registry_write_effect_tools: Option<Arc<HashSet<String>>>,
    tool_registry_sub_agent_patch_write_extra_tools: Option<Arc<HashSet<String>>>,
    tool_registry_sub_agent_test_runner_extra_tools: Option<Arc<HashSet<String>>>,
    tool_registry_sub_agent_review_readonly_deny_tools: Option<Arc<HashSet<String>>>,
    tool_registry_background_jobs_enabled: bool,
    tool_registry_background_job_async_tools: Arc<HashSet<String>>,
    tool_registry_background_job_max_concurrent: u64,
    tool_registry_background_job_max_queued: u64,
    tool_registry_background_job_ttl_secs: u64,
    tool_registry_background_job_result_grace_secs: u64,
    tool_registry_background_job_max_entries: u64,
    tool_registry_background_job_output_buffer_bytes: u64,
    tool_registry_tool_retry_enabled: bool,
    tool_registry_tool_retry_max_attempts: u64,
    tool_registry_tool_retry_backoff_ms: u64,
    tool_registry_tool_retry_error_codes: Arc<HashSet<String>>,
    tool_registry_tool_retry_denied_tools: Arc<HashSet<String>>,
    tool_registry_disabled_tools: Option<Arc<HashSet<String>>>,
    tool_registry_disabled_tool_prefixes: Option<Arc<[String]>>,
}

struct IntentDerived {
    llm_http_auth_mode: types::LlmHttpAuthMode,
    llm_reasoning_split: bool,
}

struct LtmDerived {
    long_term_memory_enabled: bool,
    long_term_memory_vector_backend: LongTermMemoryVectorBackend,
    long_term_memory_max_entries: usize,
    long_term_memory_inject_max_chars: usize,
    long_term_memory_store_sqlite_path: String,
    long_term_memory_top_k: usize,
    long_term_memory_max_chars_per_chunk: usize,
    long_term_memory_min_chars_to_index: usize,
    long_term_memory_async_index: bool,
    long_term_memory_auto_index_turns: bool,
    long_term_memory_auto_summarize_experience: bool,
    long_term_memory_prioritize_experience_recall: bool,
    long_term_memory_default_ttl_secs: u64,
}

struct CodebaseSemanticDerived {
    codebase_semantic_search_enabled: bool,
    codebase_semantic_invalidate_on_workspace_change: bool,
    codebase_semantic_index_sqlite_path: String,
    codebase_semantic_max_file_bytes: usize,
    codebase_semantic_chunk_max_chars: usize,
    codebase_semantic_top_k: usize,
    codebase_semantic_query_max_chunks: usize,
    codebase_semantic_rebuild_max_files: usize,
    codebase_semantic_rebuild_incremental: bool,
    codebase_semantic_hybrid_alpha: f32,
    codebase_semantic_fts_top_n: usize,
    codebase_semantic_hybrid_semantic_pool: usize,
}

/// `finalize_agent_config` 在角色目录就绪后的后半段（降低单函数 CCN）。
struct FinalizeAfterRoles {
    b: ConfigBuilder,
    tr: ToolRegistryDerived,
    intent: IntentDerived,
    ltm: LtmDerived,
    sem: CodebaseSemanticDerived,
    max_message_history: usize,
    command_timeout_secs: u64,
    command_max_output_len: usize,
    allow_external_path_with_approval: bool,
    max_tokens: u32,
    llm_context_tokens: u32,
    temperature: f32,
    api_timeout_secs: u64,
    api_max_retries: u32,
    api_retry_delay_secs: u64,
    weather_timeout_secs: u64,
    reflection_default_max_rounds: usize,
    allowed_commands: Arc<[String]>,
    workspace_allowed_roots: Vec<PathBuf>,
    web_workspace_pool: Option<PathBuf>,
    system_prompt: String,
    default_agent_role_id: Option<String>,
    agent_roles: agent_roles::AgentRoleCatalogBuilt,
    coding_workbench_enabled: bool,
    coding_workbench_increment_file: String,
    default_session_mode: crate::cm_types::SessionMode,
    system_prompt_search_bases: Vec<PathBuf>,
    run_command_working_dir: PathBuf,
    scheduled_agent_tasks: Vec<ScheduledAgentTask>,
    /// 权限等级配置（S1：只解析与投影，执行点尚未消费）。
    permission: types::PermissionConfig,
}

/// 验证、clamp 并组装最终 `AgentConfig`（实现体；`finalize` 为薄包装以降低圈复杂度扫描中的函数 CCN）。
fn finalize_agent_config(
    mut b: ConfigBuilder,
    system_prompt_search_bases: Vec<PathBuf>,
) -> Result<AgentConfig, String> {
    validate::validate_builder_numeric_ranges(&b)?;
    checks::validate_required_llm_endpoints(&b)?;
    let tr = derived::derive_tool_registry_fields(&b);
    let intent = derived::derive_intent_fields(&b)?;
    let ltm = derived::derive_ltm(&b)?;
    let sem = derived::derive_codebase_semantic(&b);
    let mid = agent_layers::clamp_finalize_mid_layer_scalars(&b);
    let allowed_commands = agent_layers::allowed_commands_arc_from_builder(&b);

    let (run_command_working_dir, workspace_allowed_roots, web_workspace_pool) =
        checks::resolve_finalize_workspace_paths(&b)?;

    let pm = agent_layers::merge_system_prompt_layers_for_finalize(
        &mut b,
        &system_prompt_search_bases,
        run_command_working_dir.as_path(),
    )?;

    let default_agent_role_id = b
        .roles_prompts
        .default_agent_role_id
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let (default_agent_role_id, agent_roles) =
        agent_roles::finalize_agent_role_catalog(agent_roles::FinalizeAgentRoleCatalogParams {
            entries: std::mem::take(&mut b.agent_role_entries),
            default_role_id: default_agent_role_id,
            global_effective_system_prompt: pm.system_prompt.as_str(),
            universal_l0_system_prompt: pm.universal_l0_system_prompt.as_str(),
            coding_workbench_increment: pm.coding_workbench_increment.as_str(),
            coding_workbench_enabled: pm.coding_workbench_enabled,
            system_prompt_search_bases: &system_prompt_search_bases,
            run_command_working_dir: run_command_working_dir.as_path(),
        })?;

    let (default_session_mode, permission) = tail::derive_session_policy(&b)?;

    let scheduled_agent_tasks = super::scheduled_agent_task::finalize_scheduled_agent_tasks(
        std::mem::take(&mut b.scheduled_agent_task_rows),
        agent_roles.as_ref(),
    )?;

    build::finalize_agent_config_tail(FinalizeAfterRoles {
        b,
        tr,
        intent,
        ltm,
        sem,
        max_message_history: mid.max_message_history,
        command_timeout_secs: mid.command_timeout_secs,
        command_max_output_len: mid.command_max_output_len,
        allow_external_path_with_approval: mid.allow_external_path_with_approval,
        max_tokens: mid.max_tokens,
        llm_context_tokens: mid.llm_context_tokens,
        temperature: mid.temperature,
        api_timeout_secs: mid.api_timeout_secs,
        api_max_retries: mid.api_max_retries,
        api_retry_delay_secs: mid.api_retry_delay_secs,
        weather_timeout_secs: mid.weather_timeout_secs,
        reflection_default_max_rounds: mid.reflection_default_max_rounds,
        allowed_commands,
        workspace_allowed_roots,
        web_workspace_pool,
        system_prompt: pm.system_prompt,
        default_agent_role_id,
        agent_roles,
        coding_workbench_enabled: pm.coding_workbench_enabled,
        coding_workbench_increment_file: pm.coding_workbench_increment_file,
        default_session_mode,
        system_prompt_search_bases,
        run_command_working_dir,
        scheduled_agent_tasks,
        permission,
    })
}

