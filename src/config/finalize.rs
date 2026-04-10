//! 将 [`super::builder::ConfigBuilder`] 校验、clamp 并组装为 [`super::types::AgentConfig`]。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::agent::per_coord::FinalPlanRequirementMode;

use super::agent_roles;
use super::builder::ConfigBuilder;
use super::cursor_rules;
use super::types::{
    self, AgentConfig, LongTermMemoryScopeMode, LongTermMemoryVectorBackend, PlannerExecutorMode,
    StagedPlanFeedbackMode, WebSearchProvider,
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
    include_str!("../../config/prompts/thinking_avoid_echo_appendix.md");

/// 与 [`resolve_thinking_avoid_echo_appendix`] 使用的内置正文一致；供 `augment_system_prompt` 等在运行时附录字段为空时回退。
pub(crate) fn embedded_thinking_avoid_echo_appendix() -> &'static str {
    EMBEDDED_THINKING_AVOID_ECHO_APPENDIX.trim()
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

/// 验证、clamp 并组装最终 `AgentConfig`。
pub(super) fn finalize(
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
    let final_plan_require_strict_workflow_node_coverage = b
        .final_plan_require_strict_workflow_node_coverage
        .unwrap_or(false);
    let final_plan_semantic_check_enabled = b.final_plan_semantic_check_enabled.unwrap_or(false);
    let final_plan_semantic_check_max_non_readonly_tools = b
        .final_plan_semantic_check_max_non_readonly_tools
        .unwrap_or(0)
        .min(32) as usize;
    let final_plan_semantic_check_max_tokens = b
        .final_plan_semantic_check_max_tokens
        .unwrap_or(256)
        .clamp(32, 1024) as u32;
    let planner_executor_mode = match b.planner_executor_mode_str.as_deref() {
        Some(s) => PlannerExecutorMode::parse(s)?,
        None => PlannerExecutorMode::default(),
    };
    let tool_message_max_chars = b
        .tool_message_max_chars
        .unwrap_or(32768)
        .clamp(1024, 1_048_576) as usize;
    let tool_result_envelope_v1 = b.tool_result_envelope_v1.unwrap_or(true);
    let agent_tool_stats_enabled = b.agent_tool_stats_enabled.unwrap_or(false);
    let agent_tool_stats_window_events = b
        .agent_tool_stats_window_events
        .unwrap_or(200)
        .clamp(16, 65_536) as usize;
    let agent_tool_stats_min_samples =
        b.agent_tool_stats_min_samples.unwrap_or(5).clamp(1, 10_000) as usize;
    let agent_tool_stats_max_chars = b
        .agent_tool_stats_max_chars
        .unwrap_or(800)
        .clamp(64, 32_768) as usize;
    let agent_tool_stats_warn_below_success_ratio = b
        .agent_tool_stats_warn_below_success_ratio
        .unwrap_or(0.65)
        .clamp(0.0, 1.0);
    let materialize_deepseek_dsml_tool_calls =
        b.materialize_deepseek_dsml_tool_calls.unwrap_or(true);
    let thinking_avoid_echo_system_prompt = b.thinking_avoid_echo_system_prompt.unwrap_or(true);
    let thinking_avoid_echo_appendix = resolve_thinking_avoid_echo_appendix(
        thinking_avoid_echo_system_prompt,
        b.thinking_avoid_echo_appendix.as_deref(),
        b.thinking_avoid_echo_appendix_file.as_deref(),
        &system_prompt_search_bases,
        run_command_working_dir.as_path(),
    )?;
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
    let staged_plan_optimizer_requires_parallel_tools = b
        .staged_plan_optimizer_requires_parallel_tools
        .unwrap_or(true);
    let staged_plan_ensemble_count = b.staged_plan_ensemble_count.unwrap_or(1).clamp(1, 3) as u8;
    let staged_plan_skip_ensemble_on_casual_prompt =
        b.staged_plan_skip_ensemble_on_casual_prompt.unwrap_or(true);
    let staged_plan_two_phase_nl_display = b.staged_plan_two_phase_nl_display.unwrap_or(false);
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
    let codebase_semantic_rebuild_incremental =
        b.codebase_semantic_rebuild_incremental.unwrap_or(true);

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
    let tool_registry_sub_agent_patch_write_extra_tools =
        b.tool_registry_sub_agent_patch_write_extra_tools.map(|v| {
            Arc::new(
                v.into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>(),
            )
        });
    let tool_registry_sub_agent_test_runner_extra_tools =
        b.tool_registry_sub_agent_test_runner_extra_tools.map(|v| {
            Arc::new(
                v.into_iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>(),
            )
        });
    let tool_registry_sub_agent_review_readonly_deny_tools = b
        .tool_registry_sub_agent_review_readonly_deny_tools
        .map(|v| {
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
        final_plan_require_strict_workflow_node_coverage,
        final_plan_semantic_check_enabled,
        final_plan_semantic_check_max_non_readonly_tools,
        final_plan_semantic_check_max_tokens,
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
        agent_tool_stats_enabled,
        agent_tool_stats_window_events,
        agent_tool_stats_min_samples,
        agent_tool_stats_max_chars,
        agent_tool_stats_warn_below_success_ratio,
        materialize_deepseek_dsml_tool_calls,
        thinking_avoid_echo_system_prompt,
        thinking_avoid_echo_appendix,
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
        staged_plan_optimizer_requires_parallel_tools,
        staged_plan_ensemble_count,
        staged_plan_skip_ensemble_on_casual_prompt,
        staged_plan_two_phase_nl_display,
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
        codebase_semantic_rebuild_incremental,
        tool_registry_http_fetch_wall_timeout_secs,
        tool_registry_http_request_wall_timeout_secs,
        tool_registry_parallel_wall_timeout_secs,
        tool_registry_parallel_sync_denied_tools,
        tool_registry_parallel_sync_denied_prefixes,
        tool_registry_sync_default_inline_tools,
        tool_registry_write_effect_tools,
        tool_registry_sub_agent_patch_write_extra_tools,
        tool_registry_sub_agent_test_runner_extra_tools,
        tool_registry_sub_agent_review_readonly_deny_tools,
    })
}
