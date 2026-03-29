use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct ConfigFile {
    pub(super) agent: Option<AgentSection>,
}

#[derive(Debug, Deserialize)]
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
    /// MiniMax 等：`reasoning_split` 与 OpenAI 兼容扩展。
    pub(super) llm_reasoning_split: Option<bool>,
    /// 将 `system` 折叠进 `user`（MiniMax 线上常见拒收独立 `system`）。
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
    pub(super) cursor_rules_enabled: Option<bool>,
    pub(super) cursor_rules_dir: Option<String>,
    pub(super) cursor_rules_include_agents_md: Option<bool>,
    pub(super) cursor_rules_max_chars: Option<u64>,
    pub(super) tool_message_max_chars: Option<u64>,
    pub(super) tool_result_envelope_v1: Option<bool>,
    pub(super) materialize_deepseek_dsml_tool_calls: Option<bool>,
    pub(super) context_char_budget: Option<u64>,
    pub(super) context_min_messages_after_system: Option<u64>,
    pub(super) context_summary_trigger_chars: Option<u64>,
    pub(super) context_summary_tail_messages: Option<u64>,
    pub(super) context_summary_max_tokens: Option<u64>,
    pub(super) context_summary_transcript_max_chars: Option<u64>,
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
    /// 逻辑多规划员份数上限（1–3）。`AGENT_STAGED_PLAN_ENSEMBLE_COUNT`
    pub(super) staged_plan_ensemble_count: Option<u64>,
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
}

/// 读取 [agent] 段，缺失字段保持为 None。
/// TOML 解析失败时返回 `Err`，便于调用方区分「合法 TOML 但无 [agent]」与「格式错误」。
pub(super) fn parse_agent_section(s: &str) -> Result<Option<AgentSection>, toml::de::Error> {
    Ok(toml::from_str::<ConfigFile>(s)?.agent)
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
        let toml = r#"
[other]
key = "value"
"#;
        let result = parse_agent_section(toml).expect("should parse valid TOML");
        assert!(result.is_none(), "no [agent] section should yield None");
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
