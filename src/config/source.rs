use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigFile {
    pub(super) agent: Option<AgentSection>,
    /// 与 `config/agent_roles.toml` 同形：顶层 `[[agent_roles]]` 表数组
    #[serde(default)]
    pub(super) agent_roles: Vec<AgentRoleRow>,
    /// 可选 `[tool_registry]`：工具分发超时、并行策略等（见 `config/tools.toml`）
    #[serde(default)]
    pub(super) tool_registry: Option<ToolRegistrySection>,
}

/// `config/tools.toml` / 用户 `config.toml` 中 **`[tool_registry]`** 段（与 `[agent]` 并列）。
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolRegistrySection {
    /// `http_fetch` / `http_request` 在 **`spawn_blocking` 外圈** `tokio::time::timeout` 上限（秒）；省略则 `max(command_timeout_secs, http_fetch_timeout_secs)`。
    #[serde(default)]
    pub(super) http_fetch_wall_timeout_secs: Option<u64>,
    #[serde(default)]
    pub(super) http_request_wall_timeout_secs: Option<u64>,
    /// 按执行类覆盖 **并行只读批 / SyncDefault spawn** 墙上时钟（秒）。键与 `ToolExecutionClass` 蛇形一致，如 `http_fetch_spawn_timeout`、`blocking_sync`。
    #[serde(default)]
    pub(super) parallel_wall_timeout_secs: HashMap<String, u64>,
    /// 禁止与其它只读工具同批并行的工具名（精确匹配）；省略则用内建默认表。
    pub(super) parallel_sync_denied_tools: Option<Vec<String>>,
    /// 禁止并行批的工具名前缀；省略则用内建默认前缀规则。
    pub(super) parallel_sync_denied_prefixes: Option<Vec<String>>,
    /// 在当前 async 任务上**内联**执行的 SyncDefault 工具名（跳过 `spawn_blocking`）；省略则仅 `get_current_time`、`convert_units`。
    pub(super) sync_default_inline_tools: Option<Vec<String>>,
    /// 视为「有写副作用」的工具名（`is_readonly_tool` 为假）；省略则用内建默认表。
    pub(super) write_effect_tools: Option<Vec<String>>,
}

/// 与 `config/agent_roles.toml` 中 `[[agent_roles]]` 一行对应
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentRoleRow {
    pub(super) id: String,
    pub(super) system_prompt: Option<String>,
    pub(super) system_prompt_file: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentSection {
    pub(super) api_base: Option<String>,
    pub(super) model: Option<String>,
    /// `bearer`（默认）| `none`（不向 chat/models 发 Authorization；可不设 API_KEY）
    pub(super) llm_http_auth_mode: Option<String>,
    pub(super) max_message_history: Option<u64>,
    pub(super) tui_load_session_on_start: Option<bool>,
    pub(super) tui_session_max_messages: Option<u64>,
    /// 为 `true` 时 CLI REPL 在后台构建 [`crate::runtime::workspace_session::initial_workspace_messages`]（画像 / 依赖摘要 / 可选磁盘会话）；默认 `false` 仅首条 `system`。
    pub(super) repl_initial_workspace_messages_enabled: Option<bool>,
    pub(super) command_timeout_secs: Option<u64>,
    pub(super) command_max_output_len: Option<u64>,
    pub(super) allowed_commands: Option<Vec<String>>,
    pub(super) run_command_working_dir: Option<String>,
    pub(super) max_tokens: Option<u64>,
    pub(super) temperature: Option<f64>,
    pub(super) llm_seed: Option<i64>,
    /// MiniMax 等：`reasoning_split`；`None` 时由 `finalize` 按网关推断默认（MiniMax 为 `true`）。
    pub(super) llm_reasoning_split: Option<bool>,
    /// 智谱 **bigmodel.cn** GLM-5 等：为真时请求体带 **`thinking: { "type": "enabled" }`**（深度思考，见官方文档）。
    pub(super) llm_bigmodel_thinking: Option<bool>,
    /// Moonshot **kimi-k2.5**：为真时请求体带 **`thinking: { "type": "disabled" }`**（文档默认服务端为 enabled，见 Kimi Chat API）。
    pub(super) llm_kimi_thinking_disabled: Option<bool>,
    /// 已废弃：仍解析以兼容旧 `[agent]` 配置，**运行时已忽略**。MiniMax 由源码按 **`model` / `api_base`** 自动折叠 **`system`→`user`**（见 [`crate::llm::vendor::fold_system_into_user_for_config`]）。
    #[allow(dead_code)]
    pub(super) llm_fold_system_into_user: Option<bool>,
    pub(super) api_timeout_secs: Option<u64>,
    pub(super) api_max_retries: Option<u64>,
    pub(super) api_retry_delay_secs: Option<u64>,
    pub(super) weather_timeout_secs: Option<u64>,
    pub(super) web_search_provider: Option<String>,
    pub(super) web_search_api_key: Option<String>,
    pub(super) web_search_timeout_secs: Option<u64>,
    pub(super) web_search_max_results: Option<u64>,
    pub(super) http_fetch_allowed_prefixes: Option<Vec<String>>,
    pub(super) http_fetch_timeout_secs: Option<u64>,
    pub(super) http_fetch_max_response_bytes: Option<u64>,
    pub(super) reflection_default_max_rounds: Option<u64>,
    /// `never` / `workflow_reflection` / `always`
    pub(super) final_plan_requirement: Option<String>,
    pub(super) plan_rewrite_max_attempts: Option<u64>,
    /// `single_agent` / `logical_dual_agent`
    pub(super) planner_executor_mode: Option<String>,
    pub(super) system_prompt: Option<String>,
    pub(super) system_prompt_file: Option<String>,
    /// 未指定 Web/CLI `agent_role` 时使用的默认角色 id（须存在于角色表）
    pub(super) default_agent_role: Option<String>,
    pub(super) cursor_rules_enabled: Option<bool>,
    pub(super) cursor_rules_dir: Option<String>,
    pub(super) cursor_rules_include_agents_md: Option<bool>,
    pub(super) cursor_rules_max_chars: Option<u64>,
    pub(super) tool_message_max_chars: Option<u64>,
    pub(super) tool_result_envelope_v1: Option<bool>,
    pub(super) agent_tool_stats_enabled: Option<bool>,
    pub(super) agent_tool_stats_window_events: Option<u64>,
    pub(super) agent_tool_stats_min_samples: Option<u64>,
    pub(super) agent_tool_stats_max_chars: Option<u64>,
    pub(super) agent_tool_stats_warn_below_success_ratio: Option<f64>,
    pub(super) materialize_deepseek_dsml_tool_calls: Option<bool>,
    pub(super) context_char_budget: Option<u64>,
    pub(super) context_min_messages_after_system: Option<u64>,
    pub(super) context_summary_trigger_chars: Option<u64>,
    pub(super) context_summary_tail_messages: Option<u64>,
    pub(super) context_summary_max_tokens: Option<u64>,
    pub(super) context_summary_transcript_max_chars: Option<u64>,
    pub(super) health_llm_models_probe: Option<bool>,
    pub(super) health_llm_models_probe_cache_secs: Option<u64>,
    pub(super) chat_queue_max_concurrent: Option<u64>,
    pub(super) chat_queue_max_pending: Option<u64>,
    /// 单轮并行只读 eligible 工具批时 `spawn_blocking` 最大并发；默认与 `chat_queue_max_concurrent` 相同。
    pub(super) parallel_readonly_tools_max: Option<u64>,
    /// `read_file` 单轮缓存容量；`0` 关闭。
    pub(super) read_file_turn_cache_max_entries: Option<u64>,
    pub(super) test_result_cache_enabled: Option<bool>,
    pub(super) test_result_cache_max_entries: Option<u64>,
    pub(super) session_workspace_changelist_enabled: Option<bool>,
    pub(super) session_workspace_changelist_max_chars: Option<u64>,
    pub(super) staged_plan_execution: Option<bool>,
    pub(super) staged_plan_phase_instruction: Option<String>,
    /// 为 true（默认）时：内置规划说明会要求模型在无具体任务时输出 `no_task` + 空 `steps`。
    pub(super) staged_plan_allow_no_task: Option<bool>,
    /// `fail_fast`（默认）或 `patch_planner`
    pub(super) staged_plan_feedback_mode: Option<String>,
    pub(super) staged_plan_patch_max_attempts: Option<u64>,
    /// CLI 是否在无工具规划轮向 stdout 打印模型原文；默认 true。`AGENT_STAGED_PLAN_CLI_SHOW_PLANNER_STREAM`
    pub(super) staged_plan_cli_show_planner_stream: Option<bool>,
    /// 首轮规划后是否再跑无工具优化轮；默认 true。`AGENT_STAGED_PLAN_OPTIMIZER_ROUND`
    pub(super) staged_plan_optimizer_round: Option<bool>,
    /// 无并行批处理内建工具时是否跳过优化轮；默认 true。`AGENT_STAGED_PLAN_OPTIMIZER_REQUIRES_PARALLEL_TOOLS`
    pub(super) staged_plan_optimizer_requires_parallel_tools: Option<bool>,
    /// 逻辑多规划员份数上限（1–3）。`AGENT_STAGED_PLAN_ENSEMBLE_COUNT`
    pub(super) staged_plan_ensemble_count: Option<u64>,
    /// 寒暄/极短用户输入时是否跳过 ensemble；默认 true。`AGENT_STAGED_PLAN_SKIP_ENSEMBLE_ON_CASUAL_PROMPT`
    pub(super) staged_plan_skip_ensemble_on_casual_prompt: Option<bool>,
    /// `none` | `docker`；`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_MODE`
    pub(super) sync_default_tool_sandbox_mode: Option<String>,
    /// Docker 沙盒镜像。`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE`
    pub(super) sync_default_tool_sandbox_docker_image: Option<String>,
    /// Docker 网络；空=none。`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK`
    pub(super) sync_default_tool_sandbox_docker_network: Option<String>,
    /// `docker run` 超时秒。`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS`
    pub(super) sync_default_tool_sandbox_docker_timeout_secs: Option<u64>,
    /// 容器 `user`：`current`（默认）、`image`、或 `uid[:gid]`。`AGENT_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER`
    pub(super) sync_default_tool_sandbox_docker_user: Option<String>,
    /// Web 工作区可选根目录；省略或空则仅允许 `run_command_working_dir` 及其子目录
    pub(super) workspace_allowed_roots: Option<Vec<String>>,
    pub(super) web_api_bearer_token: Option<String>,
    pub(super) allow_insecure_no_auth_for_non_loopback: Option<bool>,
    pub(super) conversation_store_sqlite_path: Option<String>,
    pub(super) agent_memory_file_enabled: Option<bool>,
    pub(super) agent_memory_file: Option<String>,
    pub(super) agent_memory_file_max_chars: Option<u64>,
    pub(super) project_profile_inject_enabled: Option<bool>,
    pub(super) project_profile_inject_max_chars: Option<u64>,
    pub(super) project_dependency_brief_inject_enabled: Option<bool>,
    pub(super) project_dependency_brief_inject_max_chars: Option<u64>,
    pub(super) tool_call_explain_enabled: Option<bool>,
    pub(super) tool_call_explain_min_chars: Option<u64>,
    pub(super) tool_call_explain_max_chars: Option<u64>,
    /// `conversation`（当前唯一值）
    pub(super) long_term_memory_scope_mode: Option<String>,
    /// `disabled` | `fastembed`（缺省与长期记忆默认一致）| `qdrant` | `pgvector`（后两者未接入时 `finalize` 报错）
    pub(super) long_term_memory_vector_backend: Option<String>,
    pub(super) long_term_memory_enabled: Option<bool>,
    pub(super) long_term_memory_max_entries: Option<u64>,
    pub(super) long_term_memory_inject_max_chars: Option<u64>,
    pub(super) long_term_memory_store_sqlite_path: Option<String>,
    pub(super) long_term_memory_top_k: Option<u64>,
    pub(super) long_term_memory_max_chars_per_chunk: Option<u64>,
    pub(super) long_term_memory_min_chars_to_index: Option<u64>,
    pub(super) long_term_memory_async_index: Option<bool>,
    pub(super) mcp_enabled: Option<bool>,
    pub(super) mcp_command: Option<String>,
    pub(super) mcp_tool_timeout_secs: Option<u64>,
    pub(super) codebase_semantic_search_enabled: Option<bool>,
    pub(super) codebase_semantic_invalidate_on_workspace_change: Option<bool>,
    pub(super) codebase_semantic_index_sqlite_path: Option<String>,
    pub(super) codebase_semantic_max_file_bytes: Option<u64>,
    pub(super) codebase_semantic_chunk_max_chars: Option<u64>,
    pub(super) codebase_semantic_top_k: Option<u64>,
    pub(super) codebase_semantic_query_max_chunks: Option<u64>,
    pub(super) codebase_semantic_rebuild_max_files: Option<u64>,
    pub(super) codebase_semantic_rebuild_incremental: Option<bool>,
}

/// 读取 [agent] 段，缺失字段保持为 None。
/// TOML 解析失败时返回 `Err`，便于调用方区分「合法 TOML 但无 [agent]」与「格式错误」。
pub(super) fn parse_agent_section(s: &str) -> Result<Option<AgentSection>, toml::de::Error> {
    Ok(toml::from_str::<ConfigFile>(s)?.agent)
}

/// `parse_config_file_roles` 的解析结果：`[agent]`、角色行、`[tool_registry]`。
pub(super) type ParsedConfigFileRoles = (
    Option<AgentSection>,
    Vec<AgentRoleRow>,
    Option<ToolRegistrySection>,
);

/// 解析完整 TOML（`[agent]` + 可选 `[[agent_roles]]` + 可选 `[tool_registry]`）；`agent` 缺失时仍返回角色行供合并。
pub(super) fn parse_config_file_roles(s: &str) -> Result<ParsedConfigFileRoles, toml::de::Error> {
    let f: ConfigFile = toml::from_str(s)?;
    Ok((f.agent, f.agent_roles, f.tool_registry))
}

/// 解析 **`config/tools.toml`** 形文件（`[agent]` + 可选 `[tool_registry]`，无 `agent_roles`）。
pub(super) fn parse_tools_config_bundle(
    s: &str,
) -> Result<(Option<AgentSection>, Option<ToolRegistrySection>), toml::de::Error> {
    let f: ConfigFile = toml::from_str(s)?;
    Ok((f.agent, f.tool_registry))
}

pub(super) fn parse_bool_like(s: &str) -> Option<bool> {
    let v = s.trim().to_ascii_lowercase();
    if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
        Some(true)
    } else if matches!(v.as_str(), "0" | "false" | "no" | "off") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_toml_with_agent_section() {
        let toml = r#"
[agent]
api_base = "https://api.example.com"
model = "deepseek-chat"
"#;
        let result = parse_agent_section(toml).expect("should parse valid TOML");
        let agent = result.expect("should have [agent]");
        assert_eq!(agent.api_base.as_deref(), Some("https://api.example.com"));
        assert_eq!(agent.model.as_deref(), Some("deepseek-chat"));
    }

    #[test]
    fn parse_valid_toml_without_agent_section() {
        // 顶层仅允许 `agent` / `agent_roles` / `tool_registry`；注释与空文档合法且无 `[agent]`。
        let toml = "# no tables\n";
        let result = parse_agent_section(toml).expect("should parse valid TOML");
        assert!(result.is_none(), "no [agent] section should yield None");
    }

    #[test]
    fn parse_rejects_unknown_top_level_table() {
        let toml = r#"
[other]
key = "value"
"#;
        let err = parse_agent_section(toml).expect_err("unknown top-level table should fail");
        assert!(
            err.to_string().contains("unknown field") || err.to_string().contains("other"),
            "expected unknown field error, got: {err}"
        );
    }

    #[test]
    fn parse_empty_toml() {
        let result = parse_agent_section("").expect("empty TOML is valid");
        assert!(result.is_none());
    }

    #[test]
    fn parse_malformed_toml_returns_error() {
        let bad = "[[[ not valid toml !!!";
        let result = parse_agent_section(bad);
        assert!(result.is_err(), "malformed TOML should return Err");
    }

    #[test]
    fn parse_rejects_unknown_key_in_agent_section() {
        let toml = r#"
[agent]
api_base = "https://api.example.com"
model = "m"
typo_unknown_key = 1
"#;
        let err = parse_agent_section(toml).expect_err("unknown key should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("unknown field") || msg.contains("unknown"),
            "expected serde unknown field error, got: {msg}"
        );
    }

    #[test]
    fn parse_bool_like_truthy() {
        for s in [
            "1", "true", "True", "TRUE", "yes", "YES", "on", "ON", " true ",
        ] {
            assert_eq!(parse_bool_like(s), Some(true), "expected true for {:?}", s);
        }
    }

    #[test]
    fn parse_bool_like_falsy() {
        for s in [
            "0", "false", "False", "FALSE", "no", "NO", "off", "OFF", " false ",
        ] {
            assert_eq!(
                parse_bool_like(s),
                Some(false),
                "expected false for {:?}",
                s
            );
        }
    }

    #[test]
    fn parse_bool_like_invalid() {
        assert_eq!(parse_bool_like("maybe"), None);
        assert_eq!(parse_bool_like(""), None);
    }
}
