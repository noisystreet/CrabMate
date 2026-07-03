//! 将 [`super::builder::ConfigBuilder`] 校验、clamp 并组装为 [`super::types::AgentConfig`]。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::FinalPlanRequirementMode;
use crate::OrchestrationProfile;

use super::agent_roles;
use super::builder::ConfigBuilder;
use super::cursor_rules;
use super::skills;
use super::types::{
    self, AgentConfig, LongTermMemoryScopeMode, LongTermMemoryVectorBackend, PlannerExecutorMode,
    ScheduledAgentTask, StagedPlanBaselineMode, StagedPlanFeedbackMode, WebSearchProvider,
};
use super::validate;
use super::workspace_roots;

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

/// 内置默认附录（与仓库 **`config/prompts/thinking_avoid_echo_appendix.md`** 一致；未配置路径或读盘失败时采用）。
const EMBEDDED_THINKING_AVOID_ECHO_APPENDIX: &str =
    include_str!("../../../config/prompts/thinking_avoid_echo_appendix.md");

/// 编程工作台增量（与 **`config/prompts/coding_workbench_increment.md`** 一致）。
const EMBEDDED_CODING_WORKBENCH_INCREMENT: &str =
    include_str!("../../../config/prompts/coding_workbench_increment.md");

/// 与 [`resolve_thinking_avoid_echo_appendix`] 使用的内置正文一致；供 `augment_system_prompt` 等在运行时附录字段为空时回退。
pub fn embedded_thinking_avoid_echo_appendix() -> &'static str {
    EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim()
}

/// 默认全局会话与工程向角色共用的编程层增量正文。
pub(crate) fn embedded_coding_workbench_increment() -> &'static str {
    EMBEDDED_CODING_WORKBENCH_INCREMENT.trim()
}

/// 读盘解析编程工作台增量；禁用时或读盘失败（回退嵌入默认）返回空或默认正文。
pub(crate) fn resolve_coding_workbench_increment(
    enabled: bool,
    file: &str,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    if !enabled {
        return Ok(String::new());
    }
    let file = file.trim();
    let path = if file.is_empty() {
        "config/prompts/coding_workbench_increment.md"
    } else {
        file
    };
    match read_system_prompt_file_resolved(path, config_bases, run_command_working_dir) {
        Ok(raw) => {
            let t = raw.trim().to_string();
            if t.is_empty() {
                Ok(embedded_coding_workbench_increment().to_string())
            } else {
                Ok(t)
            }
        }
        Err(_) => Ok(embedded_coding_workbench_increment().to_string()),
    }
}

fn resolve_thinking_avoid_echo_appendix(
    enabled: bool,
    inline: Option<&str>,
    file: Option<&str>,
    config_bases: &[PathBuf],
    run_command_working_dir: &Path,
) -> Result<String, String> {
    const MAX_CHARS: usize = 32_768;
    if !enabled {
        return Ok(String::new());
    }
    let body = if let Some(path) = file.map(str::trim).filter(|s| !s.is_empty()) {
        match read_system_prompt_file_resolved(path, config_bases, run_command_working_dir) {
            Ok(raw) => {
                let t = raw.trim().to_string();
                if t.is_empty() {
                    log::warn!(
                        target: "crabmate",
                        "thinking_avoid_echo_appendix_file 读盘为空，使用内置附录"
                    );
                    EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim().to_string()
                } else {
                    t
                }
            }
            Err(e) => {
                log::warn!(
                    target: "crabmate",
                    "{e}，使用内置 thinking_avoid_echo 附录"
                );
                EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim().to_string()
            }
        }
    } else if let Some(s) = inline.map(str::trim).filter(|s| !s.is_empty()) {
        s.to_string()
    } else {
        EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim().to_string()
    };
    if body.chars().count() > MAX_CHARS {
        return Err(format!(
            "配置错误：thinking_avoid_echo 附录正文超过 {MAX_CHARS} 字符"
        ));
    }
    Ok(body)
}

/// `context_char_budget > 0` 且 `context_min_messages_after_system >= max_message_history` 时，按字符删旧消息往往难以生效（条数裁剪已收紧窗口）。
pub(crate) fn context_budget_vs_history_suspicious(
    max_message_history: usize,
    context_char_budget: usize,
    context_min_messages_after_system: usize,
) -> bool {
    context_char_budget > 0 && context_min_messages_after_system >= max_message_history
}

fn validate_ltm_backend_when_enabled(
    enabled: bool,
    backend: LongTermMemoryVectorBackend,
) -> Result<(), String> {
    if !enabled {
        return Ok(());
    }
    match backend {
        LongTermMemoryVectorBackend::Qdrant | LongTermMemoryVectorBackend::Pgvector => Err(
            "配置错误：长期记忆向量后端 qdrant / pgvector 尚未接入；请使用 disabled 或 fastembed，或关闭 long_term_memory_enabled"
                .to_string(),
        ),
        LongTermMemoryVectorBackend::Disabled | LongTermMemoryVectorBackend::Fastembed => Ok(()),
    }
}

fn validate_docker_sandbox_image(
    mode: types::SyncDefaultToolSandboxMode,
    image: &str,
) -> Result<(), String> {
    if mode == types::SyncDefaultToolSandboxMode::Docker && image.trim().is_empty() {
        return Err(
            "配置错误：sync_default_tool_sandbox_mode=docker 时必须设置非空的 sync_default_tool_sandbox_docker_image"
                .to_string(),
        );
    }
    Ok(())
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
}

fn derive_tool_registry_fields(b: &ConfigBuilder) -> ToolRegistryDerived {
    let tr = &b.tool_registry_policy;
    ToolRegistryDerived {
        tool_registry_http_fetch_wall_timeout_secs: tr
            .tool_registry_http_fetch_wall_timeout_secs
            .map(|s| s.clamp(1, 86_400)),
        tool_registry_http_request_wall_timeout_secs: tr
            .tool_registry_http_request_wall_timeout_secs
            .map(|s| s.clamp(1, 86_400)),
        tool_registry_parallel_wall_timeout_secs: Arc::new(
            tr.tool_registry_parallel_wall_timeout_secs
                .iter()
                .map(|(k, v)| (k.clone(), (*v).clamp(1, 86_400)))
                .collect::<HashMap<_, _>>(),
        ),
        tool_registry_parallel_sync_denied_tools: tr
            .tool_registry_parallel_sync_denied_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_parallel_sync_denied_prefixes: tr
            .tool_registry_parallel_sync_denied_prefixes
            .as_ref()
            .map(|v| {
                let cleaned: Vec<String> = v
                    .iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Arc::from(cleaned.into_boxed_slice())
            }),
        tool_registry_sync_default_inline_tools: tr
            .tool_registry_sync_default_inline_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_write_effect_tools: tr.tool_registry_write_effect_tools.as_ref().map(|v| {
            Arc::new(
                v.iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>(),
            )
        }),
        tool_registry_sub_agent_patch_write_extra_tools: tr
            .tool_registry_sub_agent_patch_write_extra_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_sub_agent_test_runner_extra_tools: tr
            .tool_registry_sub_agent_test_runner_extra_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_sub_agent_review_readonly_deny_tools: tr
            .tool_registry_sub_agent_review_readonly_deny_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
    }
}

struct IntentDerived {
    llm_http_auth_mode: types::LlmHttpAuthMode,
    llm_reasoning_split: bool,
    intent_execute_low_threshold: f32,
    intent_execute_high_threshold: f32,
    intent_non_hier_execute_low_threshold: f32,
    intent_non_hier_execute_high_threshold: f32,
    intent_mode_bias_enabled: bool,
    intent_l2_enabled: bool,
    intent_l2_min_confidence: f32,
    intent_l2_max_tokens: u32,
    intent_at_turn_start_enabled: bool,
    intent_l0_routing_boost_enabled: bool,
}

fn derive_intent_fields(b: &ConfigBuilder) -> Result<IntentDerived, String> {
    let llm_http_auth_mode = match b.llm.llm_http_auth_mode_str.as_deref() {
        Some(s) => types::LlmHttpAuthMode::parse(s)?,
        None => types::LlmHttpAuthMode::default(),
    };
    let llm_reasoning_split = b.llm_vendor.llm_reasoning_split.unwrap_or_else(|| {
        crate::gateway_hints::default_llm_reasoning_split_for_gateway(&b.llm.model, &b.llm.api_base)
    });
    let intent_execute_low_threshold = b
        .intent_routing
        .intent_execute_low_threshold
        .unwrap_or(0.2)
        .clamp(0.0, 1.0) as f32;
    let intent_execute_high_threshold = b
        .intent_routing
        .intent_execute_high_threshold
        .unwrap_or(0.45)
        .clamp(0.0, 1.0) as f32;
    let intent_execute_high_threshold =
        intent_execute_high_threshold.max(intent_execute_low_threshold);
    let intent_non_hier_execute_low_threshold = b
        .intent_routing
        .intent_non_hier_execute_low_threshold
        .unwrap_or(intent_execute_low_threshold as f64)
        .clamp(0.0, 1.0) as f32;
    let intent_non_hier_execute_high_threshold = b
        .intent_routing
        .intent_non_hier_execute_high_threshold
        .unwrap_or(intent_execute_high_threshold as f64)
        .clamp(0.0, 1.0) as f32;
    let intent_non_hier_execute_high_threshold =
        intent_non_hier_execute_high_threshold.max(intent_non_hier_execute_low_threshold);
    Ok(IntentDerived {
        llm_http_auth_mode,
        llm_reasoning_split,
        intent_execute_low_threshold,
        intent_execute_high_threshold,
        intent_non_hier_execute_low_threshold,
        intent_non_hier_execute_high_threshold,
        intent_mode_bias_enabled: b.intent_routing.intent_mode_bias_enabled.unwrap_or(true),
        intent_l2_enabled: b.intent_routing.intent_l2_enabled.unwrap_or(true),
        intent_l2_min_confidence: b
            .intent_routing
            .intent_l2_min_confidence
            .unwrap_or(0.7)
            .clamp(0.0, 1.0) as f32,
        intent_l2_max_tokens: b
            .intent_routing
            .intent_l2_max_tokens
            .unwrap_or(384)
            .clamp(32, 1024) as u32,
        intent_at_turn_start_enabled: b
            .intent_routing
            .intent_at_turn_start_enabled
            .unwrap_or(false),
        intent_l0_routing_boost_enabled: b
            .intent_routing
            .intent_l0_routing_boost_enabled
            .unwrap_or(true),
    })
}

struct LtmDerived {
    long_term_memory_enabled: bool,
    long_term_memory_scope_mode: LongTermMemoryScopeMode,
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

fn derive_ltm(b: &ConfigBuilder) -> Result<LtmDerived, String> {
    let long_term_memory_enabled = b.long_term_memory.long_term_memory_enabled.unwrap_or(true);
    let long_term_memory_scope_mode = match b
        .long_term_memory
        .long_term_memory_scope_mode_str
        .as_deref()
    {
        Some(s) => LongTermMemoryScopeMode::parse(s)?,
        None => LongTermMemoryScopeMode::default(),
    };
    let long_term_memory_vector_backend = match b
        .long_term_memory
        .long_term_memory_vector_backend_str
        .as_deref()
    {
        Some(s) => LongTermMemoryVectorBackend::parse(s)?,
        None => LongTermMemoryVectorBackend::default(),
    };
    #[cfg(not(feature = "fastembed"))]
    let long_term_memory_vector_backend = if long_term_memory_enabled
        && long_term_memory_vector_backend == LongTermMemoryVectorBackend::Fastembed
    {
        LongTermMemoryVectorBackend::Disabled
    } else {
        long_term_memory_vector_backend
    };
    validate_ltm_backend_when_enabled(long_term_memory_enabled, long_term_memory_vector_backend)?;
    Ok(LtmDerived {
        long_term_memory_enabled,
        long_term_memory_scope_mode,
        long_term_memory_vector_backend,
        long_term_memory_max_entries: b
            .long_term_memory
            .long_term_memory_max_entries
            .unwrap_or(256)
            .clamp(1, 100_000) as usize,
        long_term_memory_inject_max_chars: b
            .long_term_memory
            .long_term_memory_inject_max_chars
            .unwrap_or(8000)
            .clamp(256, 500_000) as usize,
        long_term_memory_store_sqlite_path: b
            .long_term_memory
            .long_term_memory_store_sqlite_path
            .clone()
            .unwrap_or_default(),
        long_term_memory_top_k: b
            .long_term_memory
            .long_term_memory_top_k
            .unwrap_or(8)
            .clamp(1, 64) as usize,
        long_term_memory_max_chars_per_chunk: b
            .long_term_memory
            .long_term_memory_max_chars_per_chunk
            .unwrap_or(1024)
            .clamp(256, 32_000) as usize,
        long_term_memory_min_chars_to_index: b
            .long_term_memory
            .long_term_memory_min_chars_to_index
            .unwrap_or(8)
            .clamp(0, 4096) as usize,
        long_term_memory_async_index: b
            .long_term_memory
            .long_term_memory_async_index
            .unwrap_or(true),
        long_term_memory_auto_index_turns: b
            .long_term_memory
            .long_term_memory_auto_index_turns
            .unwrap_or(true),
        long_term_memory_auto_summarize_experience: b
            .long_term_memory
            .long_term_memory_auto_summarize_experience
            .unwrap_or(true),
        long_term_memory_prioritize_experience_recall: b
            .long_term_memory
            .long_term_memory_prioritize_experience_recall
            .unwrap_or(true),
        long_term_memory_default_ttl_secs: b
            .long_term_memory
            .long_term_memory_default_ttl_secs
            .unwrap_or(0)
            .clamp(0, 365 * 86400 * 10),
    })
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

fn derive_codebase_semantic(b: &ConfigBuilder) -> CodebaseSemanticDerived {
    #[cfg(feature = "fastembed")]
    let codebase_semantic_search_enabled = b
        .codebase_semantic
        .codebase_semantic_search_enabled
        .unwrap_or(true);
    #[cfg(not(feature = "fastembed"))]
    let codebase_semantic_search_enabled = false;
    let mut codebase_semantic_hybrid_alpha = b
        .codebase_semantic
        .codebase_semantic_hybrid_alpha
        .unwrap_or(0.55_f64) as f32;
    if !codebase_semantic_hybrid_alpha.is_finite() {
        codebase_semantic_hybrid_alpha = 0.55;
    }
    codebase_semantic_hybrid_alpha = codebase_semantic_hybrid_alpha.clamp(0.0, 1.0);
    CodebaseSemanticDerived {
        codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change: b
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change
            .unwrap_or(true),
        codebase_semantic_index_sqlite_path: b
            .codebase_semantic
            .codebase_semantic_index_sqlite_path
            .clone()
            .unwrap_or_default(),
        codebase_semantic_max_file_bytes: b
            .codebase_semantic
            .codebase_semantic_max_file_bytes
            .unwrap_or(512 * 1024)
            .clamp(4096, 4 * 1024 * 1024) as usize,
        codebase_semantic_chunk_max_chars: b
            .codebase_semantic
            .codebase_semantic_chunk_max_chars
            .unwrap_or(1200)
            .clamp(256, 16_000) as usize,
        codebase_semantic_top_k: b
            .codebase_semantic
            .codebase_semantic_top_k
            .unwrap_or(8)
            .clamp(1, 64) as usize,
        codebase_semantic_query_max_chunks: b
            .codebase_semantic
            .codebase_semantic_query_max_chunks
            .unwrap_or(50_000)
            .clamp(0, 2_000_000) as usize,
        codebase_semantic_rebuild_max_files: b
            .codebase_semantic
            .codebase_semantic_rebuild_max_files
            .unwrap_or(2000)
            .clamp(1, 100_000) as usize,
        codebase_semantic_rebuild_incremental: b
            .codebase_semantic
            .codebase_semantic_rebuild_incremental
            .unwrap_or(true),
        codebase_semantic_hybrid_alpha,
        codebase_semantic_fts_top_n: b
            .codebase_semantic
            .codebase_semantic_fts_top_n
            .unwrap_or(400)
            .clamp(1, 10_000) as usize,
        codebase_semantic_hybrid_semantic_pool: b
            .codebase_semantic
            .codebase_semantic_hybrid_semantic_pool
            .unwrap_or(256)
            .clamp(1, 10_000) as usize,
    }
}

/// `finalize_agent_config` 在角色目录就绪后的后半段（降低单函数 CCN）。
struct FinalizeAfterRoles {
    b: ConfigBuilder,
    tr: ToolRegistryDerived,
    intent: IntentDerived,
    ltm: LtmDerived,
    sem: CodebaseSemanticDerived,
    max_message_history: usize,
    tui_load_session_on_start: bool,
    tui_session_max_messages: usize,
    repl_initial_workspace_messages_enabled: bool,
    command_timeout_secs: u64,
    command_max_output_len: usize,
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
    system_prompt: String,
    default_agent_role_id: Option<String>,
    agent_roles: agent_roles::AgentRoleCatalogBuilt,
    coding_workbench_enabled: bool,
    coding_workbench_increment_file: String,
    system_prompt_search_bases: Vec<PathBuf>,
    run_command_working_dir: PathBuf,
    scheduled_agent_tasks: Vec<ScheduledAgentTask>,
}

fn validate_required_llm_endpoints(b: &ConfigBuilder) -> Result<(), String> {
    if b.llm.api_base.is_empty() {
        return Err("配置错误：未设置 api_base（请在 config/default_config.toml、config.toml、.agent_demo.toml 或环境变量 CM_API_BASE 中设置）".to_string());
    }
    if b.llm.model.is_empty() {
        return Err("配置错误：未设置 model（请在 config/default_config.toml、config.toml、.agent_demo.toml 或环境变量 CM_MODEL 中设置）".to_string());
    }
    Ok(())
}

fn canonical_run_command_working_dir(b: &ConfigBuilder) -> Result<PathBuf, String> {
    let raw = b
        .command_exec
        .run_command_working_dir
        .clone()
        .ok_or("配置错误：未设置 run_command_working_dir（请在 config/tools.toml、config.toml、.agent_demo.toml 或环境变量 CM_RUN_COMMAND_WORKING_DIR 中设置）")?;
    let p = Path::new(&raw);
    let p = match p.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            return Err(format!(
                "配置错误：run_command_working_dir \"{}\" 不存在或无法解析: {}",
                p.display(),
                e
            ));
        }
    };
    if !p.is_dir() {
        return Err(format!(
            "配置错误：run_command_working_dir \"{}\" 不是目录",
            p.display()
        ));
    }
    Ok(p)
}

include!("finalize_parts/finalize_agent_layers.inc.rs");

/// 验证、clamp 并组装最终 `AgentConfig`（实现体；`finalize` 为薄包装以降低圈复杂度扫描中的函数 CCN）。
fn finalize_agent_config(
    mut b: ConfigBuilder,
    system_prompt_search_bases: Vec<PathBuf>,
) -> Result<AgentConfig, String> {
    validate::validate_builder_numeric_ranges(&b)?;
    validate_required_llm_endpoints(&b)?;
    let tr = derive_tool_registry_fields(&b);
    let intent = derive_intent_fields(&b)?;
    let ltm = derive_ltm(&b)?;
    let sem = derive_codebase_semantic(&b);
    let mid = clamp_finalize_mid_layer_scalars(&b);
    let allowed_commands = allowed_commands_arc_from_builder(&b);

    let run_command_working_dir = canonical_run_command_working_dir(&b)?;

    let workspace_allowed_roots = workspace_roots::resolve_workspace_allowed_roots(
        b.workspace_roots.workspace_allowed_roots.clone(),
        run_command_working_dir.as_path(),
    )?;

    let pm = merge_system_prompt_layers_for_finalize(
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
            cursor_rules_enabled: pm.cursor_rules_enabled,
            cursor_rules_dir: &pm.cursor_rules_dir,
            cursor_rules_include_agents_md: pm.cursor_rules_include_agents_md,
            cursor_rules_max_chars: pm.cursor_rules_max_chars as usize,
            skills_enabled: pm.skills_enabled,
            skills_dir: &pm.skills_dir,
            skills_max_chars: pm.skills_max_chars as usize,
            skills_top_k: pm.skills_top_k,
        })?;

    let scheduled_agent_tasks = super::scheduled_agent_task::finalize_scheduled_agent_tasks(
        std::mem::take(&mut b.scheduled_agent_task_rows),
        agent_roles.as_ref(),
    )?;

    finalize_agent_config_tail(FinalizeAfterRoles {
        b,
        tr,
        intent,
        ltm,
        sem,
        max_message_history: mid.max_message_history,
        tui_load_session_on_start: mid.tui_load_session_on_start,
        tui_session_max_messages: mid.tui_session_max_messages,
        repl_initial_workspace_messages_enabled: mid.repl_initial_workspace_messages_enabled,
        command_timeout_secs: mid.command_timeout_secs,
        command_max_output_len: mid.command_max_output_len,
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
        system_prompt: pm.system_prompt,
        default_agent_role_id,
        agent_roles,
        coding_workbench_enabled: pm.coding_workbench_enabled,
        coding_workbench_increment_file: pm.coding_workbench_increment_file,
        system_prompt_search_bases,
        run_command_working_dir,
        scheduled_agent_tasks,
    })
}

include!("finalize_parts/finalize_tail.rs");
include!("finalize_parts/finalize_build.rs");
