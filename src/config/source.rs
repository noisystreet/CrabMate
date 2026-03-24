use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(super) struct ConfigFile {
    pub(super) agent: Option<AgentSection>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentSection {
    pub(super) api_base: Option<String>,
    pub(super) model: Option<String>,
    pub(super) max_message_history: Option<u64>,
    pub(super) tui_load_session_on_start: Option<bool>,
    pub(super) tui_session_max_messages: Option<u64>,
    pub(super) command_timeout_secs: Option<u64>,
    pub(super) command_max_output_len: Option<u64>,
    pub(super) allowed_commands: Option<Vec<String>>,
    pub(super) run_command_working_dir: Option<String>,
    pub(super) max_tokens: Option<u64>,
    pub(super) temperature: Option<f64>,
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
    pub(super) env: Option<String>,
    pub(super) allowed_commands_dev: Option<Vec<String>>,
    pub(super) allowed_commands_prod: Option<Vec<String>>,
    pub(super) tool_message_max_chars: Option<u64>,
    pub(super) context_char_budget: Option<u64>,
    pub(super) context_min_messages_after_system: Option<u64>,
    pub(super) context_summary_trigger_chars: Option<u64>,
    pub(super) context_summary_tail_messages: Option<u64>,
    pub(super) context_summary_max_tokens: Option<u64>,
    pub(super) context_summary_transcript_max_chars: Option<u64>,
    pub(super) chat_queue_max_concurrent: Option<u64>,
    pub(super) chat_queue_max_pending: Option<u64>,
    pub(super) staged_plan_execution: Option<bool>,
    pub(super) staged_plan_phase_instruction: Option<String>,
    /// Web 工作区可选根目录；省略或空则仅允许 `run_command_working_dir` 及其子目录
    pub(super) workspace_allowed_roots: Option<Vec<String>>,
    pub(super) web_api_bearer_token: Option<String>,
    pub(super) allow_insecure_no_auth_for_non_loopback: Option<bool>,
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
