//! 运行配置：API 地址、模型等，从 default_config.toml + 可选覆盖

pub mod cli;
mod cursor_rules;
mod source;
mod types;
mod workspace_roots;

use crate::agent::per_coord::FinalPlanRequirementMode;
use source::{AgentSection, parse_agent_section, parse_bool_like};
use std::path::Path;
pub use types::{AgentConfig, PlannerExecutorMode, WebSearchProvider};

/// 编译时嵌入的默认配置（与项目根 default_config.toml 一致）
const DEFAULT_CONFIG: &str = include_str!("../../default_config.toml");

/// 配置累加器：依次接受 default_config → 用户配置文件 → 环境变量的覆盖，最终 `finalize` 为 `AgentConfig`。
#[derive(Default)]
struct ConfigBuilder {
    api_base: String,
    model: String,
    system_prompt: String,
    system_prompt_file: Option<String>,
    max_message_history: Option<u64>,
    tui_load_session_on_start: Option<bool>,
    tui_session_max_messages: Option<u64>,
    command_timeout_secs: Option<u64>,
    command_max_output_len: Option<u64>,
    allowed_commands: Option<Vec<String>>,
    allowed_commands_dev: Option<Vec<String>>,
    allowed_commands_prod: Option<Vec<String>>,
    run_command_working_dir: Option<String>,
    max_tokens: Option<u64>,
    temperature: Option<f64>,
    llm_seed: Option<i64>,
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
    env_tag: Option<String>,
    tool_message_max_chars: Option<u64>,
    context_char_budget: Option<u64>,
    context_min_messages_after_system: Option<u64>,
    context_summary_trigger_chars: Option<u64>,
    context_summary_tail_messages: Option<u64>,
    context_summary_max_tokens: Option<u64>,
    context_summary_transcript_max_chars: Option<u64>,
    chat_queue_max_concurrent: Option<u64>,
    chat_queue_max_pending: Option<u64>,
    parallel_readonly_tools_max: Option<u64>,
    staged_plan_execution: Option<bool>,
    staged_plan_phase_instruction: Option<String>,
    workspace_allowed_roots: Option<Vec<String>>,
    web_api_bearer_token: Option<String>,
    allow_insecure_no_auth_for_non_loopback: Option<bool>,
    conversation_store_sqlite_path: Option<String>,
    agent_memory_file_enabled: Option<bool>,
    agent_memory_file: Option<String>,
    agent_memory_file_max_chars: Option<u64>,
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
        override_string(&mut self.system_prompt, agent.system_prompt);
        override_opt_string_non_empty(&mut self.system_prompt_file, agent.system_prompt_file);
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
        override_opt_string_non_empty(&mut self.env_tag, agent.env);

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
        override_opt_vec(&mut self.allowed_commands_dev, &agent.allowed_commands_dev);
        override_opt_vec(
            &mut self.allowed_commands_prod,
            &agent.allowed_commands_prod,
        );
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
        self.command_timeout_secs = agent.command_timeout_secs.or(self.command_timeout_secs);
        self.command_max_output_len = agent.command_max_output_len.or(self.command_max_output_len);
        self.max_tokens = agent.max_tokens.or(self.max_tokens);
        self.temperature = agent.temperature.or(self.temperature);
        self.llm_seed = agent.llm_seed.or(self.llm_seed);
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
        self.chat_queue_max_concurrent = agent
            .chat_queue_max_concurrent
            .or(self.chat_queue_max_concurrent);
        self.chat_queue_max_pending = agent.chat_queue_max_pending.or(self.chat_queue_max_pending);
        self.parallel_readonly_tools_max = agent
            .parallel_readonly_tools_max
            .or(self.parallel_readonly_tools_max);
        self.staged_plan_execution = agent.staged_plan_execution.or(self.staged_plan_execution);
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
    }
}

/// 加载配置：嵌入的 default 为底，再被配置文件覆盖，最后被环境变量覆盖。
/// 若指定 `config_path`，则只从该文件读取覆盖；否则依次尝试 config.toml、.agent_demo.toml。
/// 若最终 api_base、model 或任一运行参数仍未设置则返回错误。
pub fn load_config(config_path: Option<&str>) -> Result<AgentConfig, String> {
    let mut b = ConfigBuilder::default();

    // ── 1. 嵌入的默认配置 ──
    if let Some(agent) = parse_agent_section(DEFAULT_CONFIG)
        .expect("embedded default_config.toml must be valid TOML")
    {
        b.apply_section(agent);
    }

    // ── 2. 用户配置文件覆盖 ──
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
                b.apply_section(agent);
            }
            if config_path.is_some() {
                break;
            }
        } else if config_path.is_some() {
            return Err(format!("配置文件 \"{}\" 不存在", path));
        }
    }

    // ── 3. 环境变量覆盖（优先级最高） ──
    apply_env_overrides(&mut b);

    // ── 4. 验证与最终转换 ──
    finalize(b)
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
        }
    }
    if let Ok(p) = std::env::var("AGENT_SYSTEM_PROMPT_FILE") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            b.system_prompt_file = Some(p);
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
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_EXECUTION")
        && let Some(val) = parse_bool_like(&v)
    {
        b.staged_plan_execution = Some(val);
    }
    if let Ok(v) = std::env::var("AGENT_STAGED_PLAN_PHASE_INSTRUCTION") {
        b.staged_plan_phase_instruction = Some(v);
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
}

/// 验证、clamp 并组装最终 `AgentConfig`。
fn finalize(b: ConfigBuilder) -> Result<AgentConfig, String> {
    if b.api_base.is_empty() {
        return Err("配置错误：未设置 api_base（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_BASE 中设置）".to_string());
    }
    if b.model.is_empty() {
        return Err("配置错误：未设置 model（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_MODEL 中设置）".to_string());
    }
    let max_message_history = b.max_message_history.unwrap_or(32).clamp(1, 1024) as usize;
    let tui_load_session_on_start = b.tui_load_session_on_start.unwrap_or(false);
    let tui_session_max_messages =
        b.tui_session_max_messages.unwrap_or(400).clamp(2, 50_000) as usize;
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

    let allowed_commands_vec = if let Some(env) = b.env_tag.as_deref() {
        match env {
            "dev" => b
                .allowed_commands_dev
                .or_else(|| b.allowed_commands.clone()),
            "prod" => b
                .allowed_commands_prod
                .or_else(|| b.allowed_commands.clone()),
            _ => b.allowed_commands,
        }
    } else {
        b.allowed_commands
    }
    .unwrap_or_else(|| {
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
        b.workspace_allowed_roots,
        run_command_working_dir.as_path(),
    )?;

    let system_prompt = if let Some(path) = b.system_prompt_file {
        let path = Path::new(&path);
        std::fs::read_to_string(path)
            .map_err(|e| format!("无法读取 system_prompt_file \"{}\": {}", path.display(), e))?
    } else {
        b.system_prompt
    };
    if system_prompt.trim().is_empty() {
        return Err("配置错误：未设置 system_prompt 或 system_prompt_file".to_string());
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
    let context_char_budget = b.context_char_budget.unwrap_or(0).min(50_000_000) as usize;
    let context_min_messages_after_system = b
        .context_min_messages_after_system
        .unwrap_or(4)
        .clamp(1, 128) as usize;
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
    let chat_queue_max_concurrent = b.chat_queue_max_concurrent.unwrap_or(2).clamp(1, 256) as usize;
    let chat_queue_max_pending = b.chat_queue_max_pending.unwrap_or(32).clamp(1, 8192) as usize;
    let parallel_readonly_tools_max = b
        .parallel_readonly_tools_max
        .map(|n| n as usize)
        .unwrap_or(chat_queue_max_concurrent)
        .clamp(1, 256);
    let staged_plan_execution = b.staged_plan_execution.unwrap_or(true);
    let staged_plan_phase_instruction = b.staged_plan_phase_instruction.unwrap_or_default();
    let web_api_bearer_token = b.web_api_bearer_token.unwrap_or_default();
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

    let web_search_provider = match b.web_search_provider_str.as_deref() {
        Some(s) => WebSearchProvider::parse(s)?,
        None => WebSearchProvider::default(),
    };
    let web_search_api_key = b.web_search_api_key.unwrap_or_default();
    let web_search_timeout_secs = b.web_search_timeout_secs.unwrap_or(30).max(1);
    let web_search_max_results = b.web_search_max_results.unwrap_or(8).clamp(1, 20) as u32;

    let http_fetch_allowed_prefixes = b.http_fetch_allowed_prefixes.unwrap_or_default();
    let http_fetch_timeout_secs = b.http_fetch_timeout_secs.unwrap_or(30).max(1);
    let http_fetch_max_response_bytes = b
        .http_fetch_max_response_bytes
        .unwrap_or(524_288)
        .clamp(1024, 4_194_304) as usize;

    Ok(AgentConfig {
        api_base: b.api_base,
        model: b.model,
        max_message_history,
        tui_load_session_on_start,
        tui_session_max_messages,
        command_timeout_secs,
        command_max_output_len,
        allowed_commands,
        run_command_working_dir: run_command_working_dir.display().to_string(),
        max_tokens,
        temperature,
        llm_seed: b.llm_seed,
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
        parallel_readonly_tools_max,
        staged_plan_execution,
        staged_plan_phase_instruction,
        conversation_store_sqlite_path,
        agent_memory_file_enabled,
        agent_memory_file,
        agent_memory_file_max_chars,
    })
}
