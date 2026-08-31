use std::collections::HashMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigFile {
    pub(super) agent: Option<AgentSection>,
    /// 与 `config/agent_roles.toml` 同形：顶层 `[[agent_roles]]` 表数组
    #[serde(default)]
    pub(super) agent_roles: Vec<AgentRoleRow>,
    /// `serve` 定时对话：`[[scheduled_agent_task]]`（见 `docs/配置说明.md`）
    #[serde(default)]
    pub(super) scheduled_agent_task: Vec<ScheduledAgentTaskRow>,
    /// 可选 `[tool_registry]`：工具分发超时、并行策略等（见 `config/tools.toml`）
    #[serde(default)]
    pub(super) tool_registry: Option<ToolRegistrySection>,
}

/// `[[scheduled_agent_task]]` 单行（用户 `config.toml` 顶层表数组）
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct ScheduledAgentTaskRow {
    /// 任务标识（日志与固定会话 `conversation_id` 派生用）
    pub(super) id: String,
    /// Cron 表达式：`tokio-cron-scheduler` / **croner** 六段（秒 分 时 日 月 星期），UTC
    pub(super) schedule: String,
    /// 每轮注入的用户消息正文（等同 `POST /chat` 的 `message`）
    pub(super) message: String,
    #[serde(default = "scheduled_agent_task_enabled_default")]
    pub(super) enabled: bool,
    /// 非空则固定写入该 `conversation_id`（须合法 Client id）
    pub(super) conversation_id: Option<String>,
    /// 为 true 且未配置 `conversation_id` 时每次触发新建会话
    #[serde(default)]
    pub(super) new_conversation: bool,
    pub(super) agent_role: Option<String>,
}

fn scheduled_agent_task_enabled_default() -> bool {
    true
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
    /// 分阶段 `executor_kind: patch_write` 在默认补丁名之外额外允许的写类工具名。
    #[serde(default)]
    pub(super) sub_agent_patch_write_extra_tools: Option<Vec<String>>,
    /// 分阶段 `executor_kind: test_runner` 在默认测试运行器与 `run_command` 之外额外允许的工具名。
    #[serde(default)]
    pub(super) sub_agent_test_runner_extra_tools: Option<Vec<String>>,
    /// 分阶段 `executor_kind: review_readonly` 下显式禁止的工具名（精确匹配，优先于只读判定）。
    #[serde(default)]
    pub(super) sub_agent_review_readonly_deny_tools: Option<Vec<String>>,
    /// 后台工具任务总开关（`run_command` 的 `async=true`）；默认 `false`。契约见 `docs/design/background_tool_jobs_contract.md`。
    #[serde(default)]
    pub(super) background_jobs_enabled: Option<bool>,
    /// 后台任务同时运行上限；超出进入 `queued`（FIFO）。默认 `4`。
    #[serde(default)]
    pub(super) background_job_max_concurrent: Option<u64>,
    /// 后台任务排队上限；超限拒绝创建。默认 `32`。
    #[serde(default)]
    pub(super) background_job_max_queued: Option<u64>,
    /// 后台任务自**创建**起算的保留时长（秒）。默认 `86400`（1 天）。
    #[serde(default)]
    pub(super) background_job_ttl_secs: Option<u64>,
    /// 后台任务终态后再保留的宽限（秒），避免"刚完成即被清"。默认 `300`。
    #[serde(default)]
    pub(super) background_job_result_grace_secs: Option<u64>,
    /// 后台任务注册表条目上限；**仅淘汰终态**条目。默认 `128`。
    #[serde(default)]
    pub(super) background_job_max_entries: Option<u64>,
    /// 工具失败透明重试总开关（默认 `false`；仅瞬时失败且只读/免审批工具，见 `tool_retry_policy` 计划）。
    #[serde(default)]
    pub(super) tool_retry_enabled: Option<bool>,
    /// 含首次的总尝试次数上限（默认 `2`；`1`=不重试）。越界由 validate 拒绝。
    #[serde(default)]
    pub(super) tool_retry_max_attempts: Option<u64>,
    /// 基础退避毫秒（默认 `250`；第 n 次 = min(backoff * 2^(n-1), 5000)）。
    #[serde(default)]
    pub(super) tool_retry_backoff_ms: Option<u64>,
    /// 允许自动重试的错误码（默认 `timeout` / `http_timeout` / `rate_limited` / `http_network_error`）；空数组 = 全部禁用。
    #[serde(default)]
    pub(super) tool_retry_error_codes: Option<Vec<String>>,
    /// 额外排除的工具名（精确匹配；默认已按只读/免审批门排除写类与交互审批类工具）。
    #[serde(default)]
    pub(super) tool_retry_denied_tools: Option<Vec<String>>,
}

/// 与 `config/agent_roles.toml` 中 `[[agent_roles]]` 一行对应
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentRoleRow {
    pub(super) id: String,
    pub(super) system_prompt: Option<String>,
    pub(super) system_prompt_file: Option<String>,
    #[serde(default)]
    pub(super) allowed_tools: Option<Vec<String>>,
    /// 为 false 时不叠加编程工作台层；省略时默认 true（仍受全局 `coding_workbench_enabled` 约束）。
    #[serde(default)]
    pub(super) prepend_coding_workbench: Option<bool>,
    /// 角色默认会话模式（`ask` / `plan` / `act`）。
    #[serde(default)]
    pub(super) default_session_mode: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AgentSection {
    pub(super) api_base: Option<String>,
    pub(super) model: Option<String>,
    pub(super) planner_model: Option<String>,
    pub(super) executor_model: Option<String>,
    /// `bearer`（默认）| `none`（不向 chat/models 发 Authorization；可不设 API_KEY）
    pub(super) llm_http_auth_mode: Option<String>,
    pub(super) max_message_history: Option<u64>,
    pub(super) command_timeout_secs: Option<u64>,
    pub(super) command_max_output_len: Option<u64>,
    pub(super) allowed_commands: Option<Vec<String>>,
    pub(super) run_command_working_dir: Option<String>,
    /// 为 `true` 时 `run_command`/`terminal_session` 的工作区外路径 / `..` 可人工审批后执行；默认 `true`。
    pub(super) allow_external_path_with_approval: Option<bool>,
    pub(super) max_tokens: Option<u64>,
    /// 模型上下文窗口 token 上限（输入+输出）；用于推导会话裁剪近似字符预算。`CM_LLM_CONTEXT_TOKENS`
    pub(super) llm_context_tokens: Option<u64>,
    pub(super) temperature: Option<f64>,
    pub(super) llm_seed: Option<i64>,
    /// MiniMax 等：`reasoning_split`；`None` 时由 `finalize` 按网关推断默认（MiniMax 为 `true`）。
    pub(super) llm_reasoning_split: Option<bool>,
    /// 智谱 **bigmodel.cn** GLM-5 等：为真时请求体带 **`thinking: { "type": "enabled" }`**（深度思考，见官方文档）。
    pub(super) llm_bigmodel_thinking: Option<bool>,
    /// Moonshot **kimi-k2.5**：为真时请求体带 **`thinking: { "type": "disabled" }`**（文档默认服务端为 enabled，见 Kimi Chat API）。
    pub(super) llm_kimi_thinking_disabled: Option<bool>,
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
    pub(super) http_fetch_user_agent: Option<String>,
    pub(super) reflection_default_max_rounds: Option<u64>,
    /// `never` / `workflow_reflection` / `always`
    pub(super) final_plan_requirement: Option<String>,
    pub(super) plan_rewrite_max_attempts: Option<u64>,
    pub(super) final_plan_require_strict_workflow_node_coverage: Option<bool>,
    pub(super) final_plan_semantic_check_enabled: Option<bool>,
    pub(super) final_plan_semantic_check_accept_legacy_text: Option<bool>,
    pub(super) final_plan_semantic_check_max_non_readonly_tools: Option<u64>,
    pub(super) final_plan_semantic_check_max_tokens: Option<u64>,
    /// 仅 `single_agent`（运行时亦强制 SingleAgent；其它值在 `PlannerExecutorMode::parse` 拒绝）
    pub(super) planner_executor_mode: Option<String>,
    pub(super) system_prompt: Option<String>,
    pub(super) system_prompt_file: Option<String>,
    /// 未指定 Web/CLI `agent_role` 时使用的默认角色 id（须存在于角色表）
    pub(super) default_agent_role: Option<String>,
    /// 默认全局会话是否叠加 `coding_workbench_increment`；`CM_CODING_WORKBENCH_ENABLED`
    pub(super) coding_workbench_enabled: Option<bool>,
    /// 编程工作台增量 Markdown 路径；`CM_CODING_WORKBENCH_INCREMENT_FILE`
    pub(super) coding_workbench_increment_file: Option<String>,
    /// 默认会话模式 ask / plan / act；`CM_DEFAULT_SESSION_MODE`
    pub(super) default_session_mode: Option<String>,
    pub(super) cursor_rules_enabled: Option<bool>,
    pub(super) cursor_rules_dir: Option<String>,
    pub(super) cursor_rules_include_agents_md: Option<bool>,
    pub(super) cursor_rules_max_chars: Option<u64>,
    pub(super) skills_enabled: Option<bool>,
    pub(super) skills_dir: Option<String>,
    pub(super) skills_user_dir: Option<String>,
    pub(super) skills_system_dir: Option<String>,
    pub(super) skills_max_chars: Option<u64>,
    pub(super) skills_top_k: Option<u64>,
    pub(super) tool_message_max_chars: Option<u64>,
    pub(super) tool_result_envelope_v1: Option<bool>,
    pub(super) sse_tool_call_include_arguments: Option<bool>,
    pub(super) agent_tool_stats_enabled: Option<bool>,
    pub(super) agent_tool_stats_window_events: Option<u64>,
    pub(super) agent_tool_stats_min_samples: Option<u64>,
    pub(super) agent_tool_stats_max_chars: Option<u64>,
    pub(super) agent_tool_stats_warn_below_success_ratio: Option<f64>,
    /// 默认 true：首条 system 末尾附思考纪律附录；`false` 关闭。
    pub(super) thinking_avoid_echo_system_prompt: Option<bool>,
    /// 附录内联正文（在 `finalize` 中：若 `thinking_avoid_echo_appendix_file` 非空则读盘优先，否则用内联）。
    pub(super) thinking_avoid_echo_appendix: Option<String>,
    /// 附录文件路径（与 `system_prompt_file` 相同解析规则）；均未设置时用编译嵌入默认。
    pub(super) thinking_avoid_echo_appendix_file: Option<String>,
    pub(super) context_char_budget: Option<u64>,
    pub(super) context_min_messages_after_system: Option<u64>,
    pub(super) context_token_trigger_percent: Option<u64>,
    pub(super) context_token_target_percent: Option<u64>,
    pub(super) context_token_safety_margin_tokens: Option<u64>,
    pub(super) context_summary_trigger_chars: Option<u64>,
    pub(super) context_summary_tail_messages: Option<u64>,
    pub(super) context_summary_max_tokens: Option<u64>,
    pub(super) context_summary_transcript_max_chars: Option<u64>,
    /// 上下文 LLM 摘要 system 文件（与 `system_prompt_file` 相同路径解析）；省略则用默认路径/嵌入。
    pub(super) context_summary_system_file: Option<String>,
    /// 上下文 LLM 摘要 user 模板文件；占位符 `{max_tokens}`（或 `{max_chars}`）与 `{transcript}`。
    pub(super) context_summary_user_file: Option<String>,
    pub(super) health_llm_models_probe: Option<bool>,
    pub(super) health_llm_models_probe_cache_secs: Option<u64>,
    pub(super) chat_queue_max_concurrent: Option<u64>,
    pub(super) chat_queue_max_pending: Option<u64>,
    /// 单轮并行只读 eligible 工具批时 `spawn_blocking` 最大并发；默认与 `chat_queue_max_concurrent` 相同。
    pub(super) parallel_readonly_tools_max: Option<u64>,
    /// `read_file` 单轮缓存容量；`0` 关闭。
    pub(super) read_file_turn_cache_max_entries: Option<u64>,
    /// 只读类 **`run_command`** 进程内缓存 TTL（秒）；`0` 关闭。
    pub(super) readonly_tool_ttl_cache_secs: Option<u64>,
    pub(super) readonly_tool_ttl_cache_max_entries: Option<u64>,
    pub(super) test_result_cache_enabled: Option<bool>,
    pub(super) test_result_cache_max_entries: Option<u64>,
    pub(super) session_workspace_changelist_enabled: Option<bool>,
    pub(super) session_workspace_changelist_max_chars: Option<u64>,

    /// `none` | `docker`；`CM_SYNC_DEFAULT_TOOL_SANDBOX_MODE`
    pub(super) sync_default_tool_sandbox_mode: Option<String>,
    /// Docker 沙盒镜像。`CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE`
    pub(super) sync_default_tool_sandbox_docker_image: Option<String>,
    /// Docker 网络；空=none。`CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK`
    pub(super) sync_default_tool_sandbox_docker_network: Option<String>,
    /// `docker run` 超时秒。`CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS`
    pub(super) sync_default_tool_sandbox_docker_timeout_secs: Option<u64>,
    /// 容器 `user`：`current`（默认）、`image`、或 `uid[:gid]`。`CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER`
    pub(super) sync_default_tool_sandbox_docker_user: Option<String>,
    /// Web 工作区可选根目录；省略或空则仅允许 `run_command_working_dir` 及其子目录
    pub(super) workspace_allowed_roots: Option<Vec<String>>,
    /// Web 项目池根目录；配置后浏览器可用项目名切换/新建子工作区。须同时配置非空 `workspace_allowed_roots`。`CM_WEB_WORKSPACE_POOL`
    pub(super) web_workspace_pool: Option<String>,
    pub(super) web_api_bearer_token: Option<String>,
    /// `CM_WEB_API_REQUIRE_BEARER`；未在 TOML/环境显式设置时，finalize 默认 **false**（允许无密钥启动 `serve`）；显式 **`true`** 时须配非空 `web_api_bearer_token` 后 `serve` 才启动。
    pub(super) web_api_require_bearer: Option<bool>,
    /// 跨 Origin 允许的 Origin 列表。省略时默认含官方壳 Origin（见 `DEFAULT_SHELL_CORS_ORIGINS`）；
    /// 显式空列表关闭 CORS。`CM_WEB_CORS_ALLOWED_ORIGINS`（逗号分隔）可追加或显式清空。
    pub(super) web_cors_allowed_origins: Option<Vec<String>>,
    pub(super) allow_insecure_no_auth_for_non_loopback: Option<bool>,
    /// `CM_WEB_AUDIT_LOG_WRITE_TOOLS`；默认 true：成功执行的写副作用工具记一行结构化审计日志。
    pub(super) web_audit_log_write_tools: Option<bool>,
    /// `CM_WEB_AUDIT_TRUST_X_FORWARDED_FOR`；默认 false：若 true，客户端 IP 优先取 `X-Forwarded-For` 首跳（仅可信反向代理后启用）。
    pub(super) web_audit_trust_x_forwarded_for: Option<bool>,
    pub(super) conversation_store_sqlite_path: Option<String>,
    pub(super) agent_memory_file_enabled: Option<bool>,
    pub(super) agent_memory_file: Option<String>,
    pub(super) agent_memory_file_max_chars: Option<u64>,
    pub(super) living_docs_inject_enabled: Option<bool>,
    pub(super) living_docs_relative_dir: Option<String>,
    pub(super) living_docs_inject_max_chars: Option<u64>,
    pub(super) living_docs_file_max_each_chars: Option<u64>,
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
    pub(super) long_term_memory_auto_index_turns: Option<bool>,
    pub(super) long_term_memory_auto_summarize_experience: Option<bool>,
    pub(super) long_term_memory_prioritize_experience_recall: Option<bool>,
    pub(super) long_term_memory_default_ttl_secs: Option<u64>,
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
    pub(super) codebase_semantic_hybrid_alpha: Option<f64>,
    pub(super) codebase_semantic_fts_top_n: Option<u64>,
    pub(super) codebase_semantic_hybrid_semantic_pool: Option<u64>,
}

/// 读取 [agent] 段，缺失字段保持为 None。
/// TOML 解析失败时返回 `Err`，便于调用方区分「合法 TOML 但无 [agent]」与「格式错误」。
pub(super) fn parse_agent_section(s: &str) -> Result<Option<AgentSection>, toml::de::Error> {
    Ok(toml::from_str::<ConfigFile>(s)?.agent)
}

/// `parse_config_file_roles` 的解析结果：`[agent]`、角色行、`[tool_registry]`、定时任务行。
pub(super) type ParsedConfigFileRoles = (
    Option<AgentSection>,
    Vec<AgentRoleRow>,
    Option<ToolRegistrySection>,
    Vec<ScheduledAgentTaskRow>,
);

/// 解析完整 TOML（`[agent]` + 可选 `[[agent_roles]]` + 可选 `[tool_registry]` + 可选 `[[scheduled_agent_task]]`）；`agent` 缺失时仍返回角色行供合并。
pub(super) fn parse_config_file_roles(s: &str) -> Result<ParsedConfigFileRoles, toml::de::Error> {
    let f: ConfigFile = toml::from_str(s)?;
    Ok((
        f.agent,
        f.agent_roles,
        f.tool_registry,
        f.scheduled_agent_task,
    ))
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
    fn parse_rejects_removed_compat_keys_in_agent_section() {
        for key in [
            "llm_fold_system_into_user = true",
            "intent_mode_bias_enabled = false",
            "intent_l2_enabled = false",
            "intent_l2_min_confidence = 0.7",
            "intent_l2_max_tokens = 384",
            "intent_execute_low_threshold = 0.2",
            "intent_execute_high_threshold = 0.45",
            "intent_non_hier_execute_low_threshold = 0.2",
            "intent_non_hier_execute_high_threshold = 0.45",
            "intent_l0_routing_boost_enabled = true",
            "intent_at_turn_start_enabled = false",
            "tui_load_session_on_start = false",
            "tui_session_max_messages = 400",
            "repl_initial_workspace_messages_enabled = false",
        ] {
            let toml = format!(
                r#"
[agent]
api_base = "https://api.example.com"
model = "m"
{key}
"#
            );
            let err = parse_agent_section(&toml)
                .expect_err("removed compat key should fail deny_unknown_fields");
            let msg = err.to_string();
            assert!(
                msg.contains("unknown field") || msg.contains("unknown"),
                "expected unknown field for `{key}`, got: {msg}"
            );
        }
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
