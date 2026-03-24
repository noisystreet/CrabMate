//! 运行配置：API 地址、模型等，从 default_config.toml + 可选覆盖

pub mod cli;
mod cursor_rules;
mod source;
mod types;
mod workspace_roots;

use crate::agent::per_coord::FinalPlanRequirementMode;
use source::{parse_agent_section, parse_bool_like};
use std::path::Path;
pub use types::{AgentConfig, PlannerExecutorMode, WebSearchProvider};

/// 编译时嵌入的默认配置（与项目根 default_config.toml 一致）
const DEFAULT_CONFIG: &str = include_str!("../../default_config.toml");

/// 加载配置：嵌入的 default 为底，再被配置文件覆盖，最后被环境变量覆盖。
/// 若指定 `config_path`，则只从该文件读取覆盖；否则依次尝试 config.toml、.agent_demo.toml。
/// 若最终 api_base、model 或任一运行参数仍未设置则返回错误。
pub fn load_config(config_path: Option<&str>) -> Result<AgentConfig, String> {
    let mut api_base = String::new();
    let mut model = String::new();
    let mut max_message_history: Option<u64> = None;
    let mut tui_load_session_on_start: Option<bool> = None;
    let mut tui_session_max_messages: Option<u64> = None;
    let mut command_timeout_secs: Option<u64> = None;
    let mut command_max_output_len: Option<u64> = None;
    let mut system_prompt = String::new();
    let mut system_prompt_file: Option<String> = None;
    let mut cursor_rules_enabled: Option<bool> = None;
    let mut cursor_rules_dir: Option<String> = None;
    let mut cursor_rules_include_agents_md: Option<bool> = None;
    let mut cursor_rules_max_chars: Option<u64> = None;
    let mut max_tokens: Option<u64> = None;
    let mut temperature: Option<f64> = None;
    let mut api_timeout_secs: Option<u64> = None;
    let mut api_max_retries: Option<u64> = None;
    let mut api_retry_delay_secs: Option<u64> = None;
    let mut weather_timeout_secs: Option<u64> = None;
    let mut web_search_provider_str: Option<String> = None;
    let mut web_search_api_key: Option<String> = None;
    let mut web_search_timeout_secs: Option<u64> = None;
    let mut web_search_max_results: Option<u64> = None;
    let mut http_fetch_allowed_prefixes: Option<Vec<String>> = None;
    let mut http_fetch_timeout_secs: Option<u64> = None;
    let mut http_fetch_max_response_bytes: Option<u64> = None;
    let mut reflection_default_max_rounds: Option<u64> = None;
    let mut final_plan_requirement_str: Option<String> = None;
    let mut plan_rewrite_max_attempts: Option<u64> = None;
    let mut planner_executor_mode_str: Option<String> = None;
    let mut allowed_commands: Option<Vec<String>> = None;
    let mut allowed_commands_dev: Option<Vec<String>> = None;
    let mut allowed_commands_prod: Option<Vec<String>> = None;
    let mut run_command_working_dir: Option<String> = None;
    let mut env_tag: Option<String> = None;
    let mut tool_message_max_chars: Option<u64> = None;
    let mut context_char_budget: Option<u64> = None;
    let mut context_min_messages_after_system: Option<u64> = None;
    let mut context_summary_trigger_chars: Option<u64> = None;
    let mut context_summary_tail_messages: Option<u64> = None;
    let mut context_summary_max_tokens: Option<u64> = None;
    let mut context_summary_transcript_max_chars: Option<u64> = None;
    let mut chat_queue_max_concurrent: Option<u64> = None;
    let mut chat_queue_max_pending: Option<u64> = None;
    let mut staged_plan_execution: Option<bool> = None;
    let mut staged_plan_phase_instruction: Option<String> = None;
    let mut workspace_allowed_roots: Option<Vec<String>> = None;
    let mut web_api_bearer_token: Option<String> = None;
    let mut allow_insecure_no_auth_for_non_loopback: Option<bool> = None;

    if let Some(agent) = parse_agent_section(DEFAULT_CONFIG)
        .expect("embedded default_config.toml must be valid TOML")
    {
        api_base = agent.api_base.unwrap_or_default().trim().to_string();
        model = agent.model.unwrap_or_default().trim().to_string();
        max_message_history = agent.max_message_history.or(max_message_history);
        tui_load_session_on_start = agent
            .tui_load_session_on_start
            .or(tui_load_session_on_start);
        tui_session_max_messages = agent.tui_session_max_messages.or(tui_session_max_messages);
        command_timeout_secs = agent.command_timeout_secs.or(command_timeout_secs);
        command_max_output_len = agent.command_max_output_len.or(command_max_output_len);
        if let Some(ref v) = agent.allowed_commands
            && !v.is_empty()
        {
            allowed_commands = Some(v.clone());
        }
        if let Some(ref v) = agent.allowed_commands_dev
            && !v.is_empty()
        {
            allowed_commands_dev = Some(v.clone());
        }
        if let Some(ref v) = agent.allowed_commands_prod
            && !v.is_empty()
        {
            allowed_commands_prod = Some(v.clone());
        }
        if let Some(ref p) = agent.run_command_working_dir {
            let p = p.trim().to_string();
            if !p.is_empty() {
                run_command_working_dir = Some(p);
            }
        }
        max_tokens = agent.max_tokens.or(max_tokens);
        temperature = agent.temperature.or(temperature);
        api_timeout_secs = agent.api_timeout_secs.or(api_timeout_secs);
        api_max_retries = agent.api_max_retries.or(api_max_retries);
        api_retry_delay_secs = agent.api_retry_delay_secs.or(api_retry_delay_secs);
        weather_timeout_secs = agent.weather_timeout_secs.or(weather_timeout_secs);
        if let Some(ref s) = agent.web_search_provider {
            let s = s.trim().to_string();
            if !s.is_empty() {
                web_search_provider_str = Some(s);
            }
        }
        if let Some(ref k) = agent.web_search_api_key {
            web_search_api_key = Some(k.clone());
        }
        web_search_timeout_secs = agent.web_search_timeout_secs.or(web_search_timeout_secs);
        web_search_max_results = agent.web_search_max_results.or(web_search_max_results);
        if let Some(ref v) = agent.http_fetch_allowed_prefixes
            && !v.is_empty()
        {
            http_fetch_allowed_prefixes = Some(v.clone());
        }
        http_fetch_timeout_secs = agent.http_fetch_timeout_secs.or(http_fetch_timeout_secs);
        http_fetch_max_response_bytes = agent
            .http_fetch_max_response_bytes
            .or(http_fetch_max_response_bytes);
        reflection_default_max_rounds = agent
            .reflection_default_max_rounds
            .or(reflection_default_max_rounds);
        if let Some(ref s) = agent.final_plan_requirement {
            let s = s.trim().to_string();
            if !s.is_empty() {
                final_plan_requirement_str = Some(s);
            }
        }
        plan_rewrite_max_attempts = agent
            .plan_rewrite_max_attempts
            .or(plan_rewrite_max_attempts);
        if let Some(ref s) = agent.planner_executor_mode {
            let s = s.trim().to_string();
            if !s.is_empty() {
                planner_executor_mode_str = Some(s);
            }
        }
        if let Some(s) = agent.system_prompt {
            let s = s.trim().to_string();
            if !s.is_empty() {
                system_prompt = s;
            }
        }
        if let Some(p) = agent.system_prompt_file {
            let p = p.trim().to_string();
            if !p.is_empty() {
                system_prompt_file = Some(p);
            }
        }
        cursor_rules_enabled = agent.cursor_rules_enabled.or(cursor_rules_enabled);
        if let Some(ref p) = agent.cursor_rules_dir {
            let p = p.trim().to_string();
            if !p.is_empty() {
                cursor_rules_dir = Some(p);
            }
        }
        cursor_rules_include_agents_md = agent
            .cursor_rules_include_agents_md
            .or(cursor_rules_include_agents_md);
        cursor_rules_max_chars = agent.cursor_rules_max_chars.or(cursor_rules_max_chars);
        if let Some(e) = agent.env {
            let e = e.trim().to_string();
            if !e.is_empty() {
                env_tag = Some(e);
            }
        }
        tool_message_max_chars = agent.tool_message_max_chars.or(tool_message_max_chars);
        context_char_budget = agent.context_char_budget.or(context_char_budget);
        context_min_messages_after_system = agent
            .context_min_messages_after_system
            .or(context_min_messages_after_system);
        context_summary_trigger_chars = agent
            .context_summary_trigger_chars
            .or(context_summary_trigger_chars);
        context_summary_tail_messages = agent
            .context_summary_tail_messages
            .or(context_summary_tail_messages);
        context_summary_max_tokens = agent
            .context_summary_max_tokens
            .or(context_summary_max_tokens);
        context_summary_transcript_max_chars = agent
            .context_summary_transcript_max_chars
            .or(context_summary_transcript_max_chars);
        chat_queue_max_concurrent = agent
            .chat_queue_max_concurrent
            .or(chat_queue_max_concurrent);
        chat_queue_max_pending = agent.chat_queue_max_pending.or(chat_queue_max_pending);
        tui_load_session_on_start = agent
            .tui_load_session_on_start
            .or(tui_load_session_on_start);
        staged_plan_execution = agent.staged_plan_execution.or(staged_plan_execution);
        if let Some(ref s) = agent.staged_plan_phase_instruction {
            staged_plan_phase_instruction = Some(s.clone());
        }
        if let Some(ref v) = agent.workspace_allowed_roots
            && !v.is_empty()
        {
            workspace_allowed_roots = Some(v.clone());
        }
        if let Some(ref s) = agent.web_api_bearer_token {
            web_api_bearer_token = Some(s.trim().to_string());
        }
        allow_insecure_no_auth_for_non_loopback = agent
            .allow_insecure_no_auth_for_non_loopback
            .or(allow_insecure_no_auth_for_non_loopback);
    }

    let config_paths: Vec<&str> = match config_path {
        Some(p) => {
            let p = p.trim();
            if p.is_empty() { vec![] } else { vec![p] }
        }
        None => vec!["config.toml", ".agent_demo.toml"],
    };
    for path in config_paths {
        if Path::new(path).exists() {
            let s = std::fs::read_to_string(path)
                .map_err(|e| format!("无法读取配置文件 \"{}\": {}", path, e))?;
            let parsed = parse_agent_section(&s)
                .map_err(|e| format!("配置文件 \"{}\" TOML 解析失败: {}", path, e))?;
            if let Some(agent) = parsed {
                if let Some(a) = agent.api_base {
                    let a = a.trim().to_string();
                    if !a.is_empty() {
                        api_base = a;
                    }
                }
                if let Some(m) = agent.model {
                    let m = m.trim().to_string();
                    if !m.is_empty() {
                        model = m;
                    }
                }
                if let Some(v) = agent.max_message_history {
                    max_message_history = Some(v);
                }
                tui_load_session_on_start = agent
                    .tui_load_session_on_start
                    .or(tui_load_session_on_start);
                if let Some(v) = agent.tui_session_max_messages {
                    tui_session_max_messages = Some(v);
                }
                if let Some(v) = agent.command_timeout_secs {
                    command_timeout_secs = Some(v);
                }
                if let Some(v) = agent.command_max_output_len {
                    command_max_output_len = Some(v);
                }
                if let Some(ref v) = agent.allowed_commands
                    && !v.is_empty()
                {
                    allowed_commands = Some(v.clone());
                }
                if let Some(ref v) = agent.allowed_commands_dev
                    && !v.is_empty()
                {
                    allowed_commands_dev = Some(v.clone());
                }
                if let Some(ref v) = agent.allowed_commands_prod
                    && !v.is_empty()
                {
                    allowed_commands_prod = Some(v.clone());
                }
                if let Some(ref p) = agent.run_command_working_dir {
                    let p = p.trim().to_string();
                    if !p.is_empty() {
                        run_command_working_dir = Some(p);
                    }
                }
                if let Some(v) = agent.max_tokens {
                    max_tokens = Some(v);
                }
                if let Some(v) = agent.temperature {
                    temperature = Some(v);
                }
                if let Some(v) = agent.api_timeout_secs {
                    api_timeout_secs = Some(v);
                }
                if let Some(v) = agent.api_max_retries {
                    api_max_retries = Some(v);
                }
                if let Some(v) = agent.api_retry_delay_secs {
                    api_retry_delay_secs = Some(v);
                }
                if let Some(v) = agent.weather_timeout_secs {
                    weather_timeout_secs = Some(v);
                }
                if let Some(ref s) = agent.web_search_provider {
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        web_search_provider_str = Some(s);
                    }
                }
                if let Some(ref k) = agent.web_search_api_key {
                    web_search_api_key = Some(k.clone());
                }
                if let Some(v) = agent.web_search_timeout_secs {
                    web_search_timeout_secs = Some(v);
                }
                if let Some(v) = agent.web_search_max_results {
                    web_search_max_results = Some(v);
                }
                if let Some(ref v) = agent.http_fetch_allowed_prefixes
                    && !v.is_empty()
                {
                    http_fetch_allowed_prefixes = Some(v.clone());
                }
                if let Some(v) = agent.http_fetch_timeout_secs {
                    http_fetch_timeout_secs = Some(v);
                }
                if let Some(v) = agent.http_fetch_max_response_bytes {
                    http_fetch_max_response_bytes = Some(v);
                }
                if let Some(v) = agent.reflection_default_max_rounds {
                    reflection_default_max_rounds = Some(v);
                }
                if let Some(ref s) = agent.final_plan_requirement {
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        final_plan_requirement_str = Some(s);
                    }
                }
                if let Some(v) = agent.plan_rewrite_max_attempts {
                    plan_rewrite_max_attempts = Some(v);
                }
                if let Some(ref s) = agent.planner_executor_mode {
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        planner_executor_mode_str = Some(s);
                    }
                }
                if let Some(ss) = agent.system_prompt {
                    let ss = ss.trim().to_string();
                    if !ss.is_empty() {
                        system_prompt = ss;
                    }
                }
                if let Some(p) = agent.system_prompt_file {
                    let p = p.trim().to_string();
                    if !p.is_empty() {
                        system_prompt_file = Some(p);
                    }
                }
                cursor_rules_enabled = agent.cursor_rules_enabled.or(cursor_rules_enabled);
                if let Some(ref p) = agent.cursor_rules_dir {
                    let p = p.trim().to_string();
                    if !p.is_empty() {
                        cursor_rules_dir = Some(p);
                    }
                }
                cursor_rules_include_agents_md = agent
                    .cursor_rules_include_agents_md
                    .or(cursor_rules_include_agents_md);
                cursor_rules_max_chars = agent.cursor_rules_max_chars.or(cursor_rules_max_chars);
                if let Some(e) = agent.env {
                    let e = e.trim().to_string();
                    if !e.is_empty() {
                        env_tag = Some(e);
                    }
                }
                tool_message_max_chars = agent.tool_message_max_chars.or(tool_message_max_chars);
                context_char_budget = agent.context_char_budget.or(context_char_budget);
                context_min_messages_after_system = agent
                    .context_min_messages_after_system
                    .or(context_min_messages_after_system);
                context_summary_trigger_chars = agent
                    .context_summary_trigger_chars
                    .or(context_summary_trigger_chars);
                context_summary_tail_messages = agent
                    .context_summary_tail_messages
                    .or(context_summary_tail_messages);
                context_summary_max_tokens = agent
                    .context_summary_max_tokens
                    .or(context_summary_max_tokens);
                context_summary_transcript_max_chars = agent
                    .context_summary_transcript_max_chars
                    .or(context_summary_transcript_max_chars);
                chat_queue_max_concurrent = agent
                    .chat_queue_max_concurrent
                    .or(chat_queue_max_concurrent);
                chat_queue_max_pending = agent.chat_queue_max_pending.or(chat_queue_max_pending);
                tui_load_session_on_start = agent
                    .tui_load_session_on_start
                    .or(tui_load_session_on_start);
                staged_plan_execution = agent.staged_plan_execution.or(staged_plan_execution);
                if let Some(ref s) = agent.staged_plan_phase_instruction {
                    let s = s.trim().to_string();
                    staged_plan_phase_instruction = Some(s);
                }
                if let Some(ref v) = agent.workspace_allowed_roots
                    && !v.is_empty()
                {
                    workspace_allowed_roots = Some(v.clone());
                }
                if let Some(ref s) = agent.web_api_bearer_token {
                    web_api_bearer_token = Some(s.trim().to_string());
                }
                allow_insecure_no_auth_for_non_loopback = agent
                    .allow_insecure_no_auth_for_non_loopback
                    .or(allow_insecure_no_auth_for_non_loopback);
            }
            if config_path.is_some() {
                break;
            }
        } else if config_path.is_some() {
            return Err(format!("配置文件 \"{}\" 不存在", path));
        }
    }

    if let Ok(a) = std::env::var("AGENT_API_BASE") {
        let a = a.trim().to_string();
        if !a.is_empty() {
            api_base = a;
        }
    }
    if let Ok(m) = std::env::var("AGENT_MODEL") {
        let m = m.trim().to_string();
        if !m.is_empty() {
            model = m;
        }
    }
    if let Ok(v) = std::env::var("AGENT_MAX_MESSAGE_HISTORY")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        max_message_history = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TUI_SESSION_MAX_MESSAGES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        tui_session_max_messages = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TUI_LOAD_SESSION_ON_START") {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
            tui_load_session_on_start = Some(true);
        } else if matches!(v.as_str(), "0" | "false" | "no" | "off") {
            tui_load_session_on_start = Some(false);
        }
    }
    if let Ok(v) = std::env::var("AGENT_COMMAND_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        command_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_COMMAND_MAX_OUTPUT_LEN")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        command_max_output_len = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_ALLOWED_COMMANDS") {
        let list = v
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if !list.is_empty() {
            allowed_commands = Some(list);
        }
    }
    if let Ok(v) = std::env::var("AGENT_RUN_COMMAND_WORKING_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            run_command_working_dir = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_WORKSPACE_ALLOWED_ROOTS") {
        let list: Vec<String> = s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !list.is_empty() {
            workspace_allowed_roots = Some(list);
        }
    }
    if let Ok(v) = std::env::var("AGENT_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TEMPERATURE")
        && let Ok(n) = v.trim().parse::<f64>()
    {
        temperature = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_API_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        api_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_API_MAX_RETRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        api_max_retries = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_API_RETRY_DELAY_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        api_retry_delay_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_WEATHER_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        weather_timeout_secs = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_WEB_SEARCH_PROVIDER") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            web_search_provider_str = Some(s);
        }
    }
    if let Ok(k) = std::env::var("AGENT_WEB_SEARCH_API_KEY") {
        web_search_api_key = Some(k);
    }
    if let Ok(v) = std::env::var("AGENT_WEB_SEARCH_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        web_search_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_WEB_SEARCH_MAX_RESULTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        web_search_max_results = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_HTTP_FETCH_ALLOWED_PREFIXES") {
        let list: Vec<String> = s
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect();
        if !list.is_empty() {
            http_fetch_allowed_prefixes = Some(list);
        }
    }
    if let Ok(v) = std::env::var("AGENT_HTTP_FETCH_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        http_fetch_timeout_secs = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_HTTP_FETCH_MAX_RESPONSE_BYTES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        http_fetch_max_response_bytes = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_REFLECTION_DEFAULT_MAX_ROUNDS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        reflection_default_max_rounds = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_FINAL_PLAN_REQUIREMENT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            final_plan_requirement_str = Some(s);
        }
    }
    if let Ok(v) = std::env::var("AGENT_PLAN_REWRITE_MAX_ATTEMPTS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        plan_rewrite_max_attempts = Some(n);
    }
    if let Ok(s) = std::env::var("AGENT_PLANNER_EXECUTOR_MODE") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            planner_executor_mode_str = Some(s);
        }
    }
    if let Ok(s) = std::env::var("AGENT_SYSTEM_PROMPT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            system_prompt = s;
        }
    }
    if let Ok(p) = std::env::var("AGENT_SYSTEM_PROMPT_FILE") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            system_prompt_file = Some(p);
        }
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_ENABLED")
        && let Some(b) = parse_bool_like(&v)
    {
        cursor_rules_enabled = Some(b);
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_DIR") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            cursor_rules_dir = Some(v);
        }
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_INCLUDE_AGENTS_MD")
        && let Some(b) = parse_bool_like(&v)
    {
        cursor_rules_include_agents_md = Some(b);
    }
    if let Ok(v) = std::env::var("AGENT_CURSOR_RULES_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        cursor_rules_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_TOOL_MESSAGE_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        tool_message_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_CHAR_BUDGET")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        context_char_budget = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        context_min_messages_after_system = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_TRIGGER_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        context_summary_trigger_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_TAIL_MESSAGES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        context_summary_tail_messages = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        context_summary_max_tokens = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        context_summary_transcript_max_chars = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CHAT_QUEUE_MAX_CONCURRENT")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        chat_queue_max_concurrent = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_CHAT_QUEUE_MAX_PENDING")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        chat_queue_max_pending = Some(n);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_EXECUTION") {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
            staged_plan_execution = Some(true);
        } else if matches!(v.as_str(), "0" | "false" | "no" | "off") {
            staged_plan_execution = Some(false);
        }
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_PHASE_INSTRUCTION") {
        staged_plan_phase_instruction = Some(v);
    }
    if let Ok(v) = std::env::var("AGENT_WEB_API_BEARER_TOKEN") {
        web_api_bearer_token = Some(v.trim().to_string());
    }
    if let Ok(v) = std::env::var("AGENT_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK") {
        let v = v.trim().to_ascii_lowercase();
        if matches!(v.as_str(), "1" | "true" | "yes" | "on") {
            allow_insecure_no_auth_for_non_loopback = Some(true);
        } else if matches!(v.as_str(), "0" | "false" | "no" | "off") {
            allow_insecure_no_auth_for_non_loopback = Some(false);
        }
    }

    if api_base.is_empty() {
        return Err("配置错误：未设置 api_base（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_BASE 中设置）".to_string());
    }
    if model.is_empty() {
        return Err("配置错误：未设置 model（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_MODEL 中设置）".to_string());
    }
    let max_message_history = max_message_history.unwrap_or(32).clamp(1, 1024) as usize;
    let tui_load_session_on_start = tui_load_session_on_start.unwrap_or(false);
    let tui_session_max_messages =
        tui_session_max_messages.unwrap_or(400).clamp(2, 50_000) as usize;
    let command_timeout_secs = command_timeout_secs.unwrap_or(30).max(1);
    let command_max_output_len =
        command_max_output_len.unwrap_or(8192).clamp(1024, 131072) as usize;
    let max_tokens = max_tokens.unwrap_or(4096).clamp(256, 32768) as u32;
    let temperature = temperature.unwrap_or(0.3).clamp(0.0, 2.0) as f32;
    let api_timeout_secs = api_timeout_secs.unwrap_or(60).max(1);
    let api_max_retries = api_max_retries.unwrap_or(2).min(10) as u32;
    let api_retry_delay_secs = api_retry_delay_secs.unwrap_or(2).max(1);
    let weather_timeout_secs = weather_timeout_secs.unwrap_or(15).max(1);
    let reflection_default_max_rounds = reflection_default_max_rounds.unwrap_or(5).max(1) as usize;

    let allowed_commands = if let Some(env) = env_tag.as_deref() {
        match env {
            "dev" => allowed_commands_dev.or_else(|| allowed_commands.clone()),
            "prod" => allowed_commands_prod.or_else(|| allowed_commands.clone()),
            _ => allowed_commands,
        }
    } else {
        allowed_commands
    }
    .unwrap_or_else(|| {
        vec![
            "ls".into(),
            "pwd".into(),
            "whoami".into(),
            "date".into(),
            "echo".into(),
            "id".into(),
            "uname".into(),
            "env".into(),
            "df".into(),
            "du".into(),
            "head".into(),
            "tail".into(),
            "wc".into(),
            "cat".into(),
            "cmake".into(),
            "ninja".into(),
            "gcc".into(),
            "g++".into(),
            "clang".into(),
            "clang++".into(),
            "c++filt".into(),
            "autoreconf".into(),
            "autoconf".into(),
            "automake".into(),
            "aclocal".into(),
            "make".into(),
        ]
    });

    let run_command_working_dir = run_command_working_dir
        .ok_or("配置错误：未设置 run_command_working_dir（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_RUN_COMMAND_WORKING_DIR 中设置）")?;
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
        workspace_allowed_roots,
        run_command_working_dir.as_path(),
    )?;

    let system_prompt = if let Some(path) = system_prompt_file {
        let path = Path::new(&path);

        std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取 system_prompt_file \"{}\": {}", path.display(), e))?
    } else {
        system_prompt
    };
    if system_prompt.trim().is_empty() {
        return Err("配置错误：未设置 system_prompt 或 system_prompt_file".to_string());
    }
    let cursor_rules_enabled = cursor_rules_enabled.unwrap_or(false);
    let cursor_rules_dir = cursor_rules_dir.unwrap_or_else(|| ".cursor/rules".to_string());
    let cursor_rules_include_agents_md = cursor_rules_include_agents_md.unwrap_or(true);
    let cursor_rules_max_chars = cursor_rules_max_chars
        .unwrap_or(48_000)
        .clamp(1024, 1_000_000);
    let system_prompt = cursor_rules::merge_system_prompt_with_cursor_rules(
        system_prompt,
        cursor_rules_enabled,
        &cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars as usize,
    )?;

    let final_plan_requirement = match final_plan_requirement_str.as_deref() {
        Some(s) => FinalPlanRequirementMode::parse(s)?,
        None => FinalPlanRequirementMode::default(),
    };
    let plan_rewrite_max_attempts = plan_rewrite_max_attempts.unwrap_or(2).clamp(1, 20) as usize;
    let planner_executor_mode = match planner_executor_mode_str.as_deref() {
        Some(s) => PlannerExecutorMode::parse(s)?,
        None => PlannerExecutorMode::default(),
    };
    let tool_message_max_chars = tool_message_max_chars
        .unwrap_or(32768)
        .clamp(1024, 1_048_576) as usize;
    let context_char_budget = context_char_budget.unwrap_or(0).min(50_000_000) as usize;
    let context_min_messages_after_system =
        context_min_messages_after_system.unwrap_or(4).clamp(1, 128) as usize;
    let context_summary_trigger_chars =
        context_summary_trigger_chars.unwrap_or(0).min(50_000_000) as usize;
    let context_summary_tail_messages =
        context_summary_tail_messages.unwrap_or(12).clamp(4, 64) as usize;
    let context_summary_max_tokens =
        context_summary_max_tokens.unwrap_or(1024).clamp(256, 8192) as u32;
    let context_summary_transcript_max_chars = context_summary_transcript_max_chars
        .unwrap_or(120_000)
        .clamp(10_000, 2_000_000) as usize;
    let chat_queue_max_concurrent = chat_queue_max_concurrent.unwrap_or(2).clamp(1, 256) as usize;
    let chat_queue_max_pending = chat_queue_max_pending.unwrap_or(32).clamp(1, 8192) as usize;
    let staged_plan_execution = staged_plan_execution.unwrap_or(true);
    let staged_plan_phase_instruction = staged_plan_phase_instruction.unwrap_or_default();
    let web_api_bearer_token = web_api_bearer_token.unwrap_or_default();
    let allow_insecure_no_auth_for_non_loopback =
        allow_insecure_no_auth_for_non_loopback.unwrap_or(false);

    let web_search_provider = match web_search_provider_str.as_deref() {
        Some(s) => WebSearchProvider::parse(s)?,
        None => WebSearchProvider::default(),
    };
    let web_search_api_key = web_search_api_key.unwrap_or_default();
    let web_search_timeout_secs = web_search_timeout_secs.unwrap_or(30).max(1);
    let web_search_max_results = web_search_max_results.unwrap_or(8).clamp(1, 20) as u32;

    let http_fetch_allowed_prefixes = http_fetch_allowed_prefixes.unwrap_or_default();
    let http_fetch_timeout_secs = http_fetch_timeout_secs.unwrap_or(30).max(1);
    let http_fetch_max_response_bytes = http_fetch_max_response_bytes
        .unwrap_or(524_288)
        .clamp(1024, 4_194_304) as usize;

    Ok(AgentConfig {
        api_base,
        model,
        max_message_history,
        tui_load_session_on_start,
        tui_session_max_messages,
        command_timeout_secs,
        command_max_output_len,
        allowed_commands,
        run_command_working_dir: run_command_working_dir.display().to_string(),
        max_tokens,
        temperature,
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
        cursor_rules_enabled,
        cursor_rules_dir,
        cursor_rules_include_agents_md,
        cursor_rules_max_chars: cursor_rules_max_chars as usize,
        tool_message_max_chars,
        context_char_budget,
        context_min_messages_after_system,
        context_summary_trigger_chars,
        context_summary_tail_messages,
        context_summary_max_tokens,
        context_summary_transcript_max_chars,
        workspace_allowed_roots,
        web_api_bearer_token,
        allow_insecure_no_auth_for_non_loopback,
        chat_queue_max_concurrent,
        chat_queue_max_pending,
        staged_plan_execution,
        staged_plan_phase_instruction,
    })
}
