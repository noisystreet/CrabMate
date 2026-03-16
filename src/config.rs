//! 运行配置：API 地址、模型等，从 default_config.toml + 可选覆盖

use serde::Deserialize;
use std::path::Path;

/// 编译时嵌入的默认配置（与项目根 default_config.toml 一致）
const DEFAULT_CONFIG: &str = include_str!("../default_config.toml");

/// Agent 运行配置
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// API 基础 URL，如 https://api.deepseek.com/v1
    pub api_base: String,
    /// 模型 ID，如 deepseek-chat、deepseek-reasoner
    pub model: String,
    /// 保留的最近对话轮数（user+assistant 算一轮）
    pub max_message_history: usize,
    /// run_command 最长执行时间（秒）
    pub command_timeout_secs: u64,
    /// run_command 输出最大长度（字符），超出则截断
    pub command_max_output_len: usize,
    /// run_command 允许执行的命令白名单
    pub allowed_commands: Vec<String>,
    /// run_command 的工作目录（命令在该目录下执行）
    pub run_command_working_dir: String,
    /// 对话 API 单次请求最大 token 数
    pub max_tokens: u32,
    /// 采样温度，0～2
    pub temperature: f32,
    /// HTTP 请求超时（秒），用于 chat 等 API
    pub api_timeout_secs: u64,
    /// API 失败时最大重试次数（0 = 仅首次，不再重试）
    pub api_max_retries: u32,
    /// 重试前等待秒数（指数退避的基数）
    pub api_retry_delay_secs: u64,
    /// get_weather 工具请求超时（秒）
    pub weather_timeout_secs: u64,
    /// 系统提示词（可由 system_prompt 或 system_prompt_file 配置）
    pub system_prompt: String,
}

#[derive(Debug, Deserialize)]
struct ConfigFile {
    agent: Option<AgentSection>,
}

#[derive(Debug, Deserialize)]
struct AgentSection {
    api_base: Option<String>,
    model: Option<String>,
    max_message_history: Option<u64>,
    command_timeout_secs: Option<u64>,
    command_max_output_len: Option<u64>,
    allowed_commands: Option<Vec<String>>,
    run_command_working_dir: Option<String>,
    max_tokens: Option<u64>,
    temperature: Option<f64>,
    api_timeout_secs: Option<u64>,
    api_max_retries: Option<u64>,
    api_retry_delay_secs: Option<u64>,
    weather_timeout_secs: Option<u64>,
    system_prompt: Option<String>,
    system_prompt_file: Option<String>,
}

/// 读取 [agent] 段，缺失字段保持为 None
fn parse_agent_section(s: &str) -> Option<AgentSection> {
    toml::from_str::<ConfigFile>(s)
        .ok()?
        .agent
}

/// 加载配置：嵌入的 default 为底，再被配置文件覆盖，最后被环境变量覆盖。
/// 若指定 `config_path`，则只从该文件读取覆盖；否则依次尝试 config.toml、.agent_demo.toml。
/// 若最终 api_base、model 或任一运行参数仍未设置则返回错误。
pub fn load_config(config_path: Option<&str>) -> Result<AgentConfig, String> {
    let mut api_base = String::new();
    let mut model = String::new();
    let mut max_message_history: Option<u64> = None;
    let mut command_timeout_secs: Option<u64> = None;
    let mut command_max_output_len: Option<u64> = None;
    let mut system_prompt = String::new();
    let mut system_prompt_file: Option<String> = None;
    let mut max_tokens: Option<u64> = None;
    let mut temperature: Option<f64> = None;
    let mut api_timeout_secs: Option<u64> = None;
    let mut api_max_retries: Option<u64> = None;
    let mut api_retry_delay_secs: Option<u64> = None;
    let mut weather_timeout_secs: Option<u64> = None;
    let mut allowed_commands: Option<Vec<String>> = None;
    let mut run_command_working_dir: Option<String> = None;

    if let Some(agent) = parse_agent_section(DEFAULT_CONFIG) {
        api_base = agent.api_base.unwrap_or_default().trim().to_string();
        model = agent.model.unwrap_or_default().trim().to_string();
        max_message_history = agent.max_message_history.or(max_message_history);
        command_timeout_secs = agent.command_timeout_secs.or(command_timeout_secs);
        command_max_output_len = agent.command_max_output_len.or(command_max_output_len);
        if let Some(ref v) = agent.allowed_commands {
            if !v.is_empty() {
                allowed_commands = Some(v.clone());
            }
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
    }

    let config_paths: Vec<&str> = match config_path {
        Some(p) => {
            let p = p.trim();
            if p.is_empty() {
                vec![]
            } else {
                vec![p]
            }
        }
        None => vec!["config.toml", ".agent_demo.toml"],
    };
    for path in config_paths {
        if Path::new(path).exists() {
            let s = std::fs::read_to_string(path).map_err(|e| {
                format!("无法读取配置文件 \"{}\": {}", path, e)
            })?;
            if let Some(agent) = parse_agent_section(&s) {
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
                if let Some(v) = agent.command_timeout_secs {
                    command_timeout_secs = Some(v);
                }
                if let Some(v) = agent.command_max_output_len {
                    command_max_output_len = Some(v);
                }
                if let Some(ref v) = agent.allowed_commands {
                    if !v.is_empty() {
                        allowed_commands = Some(v.clone());
                    }
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
    if let Ok(s) = std::env::var("AGENT_MAX_MESSAGE_HISTORY") {
        if let Ok(v) = s.trim().parse::<u64>() {
            max_message_history = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_COMMAND_TIMEOUT_SECS") {
        if let Ok(v) = s.trim().parse::<u64>() {
            command_timeout_secs = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_COMMAND_MAX_OUTPUT_LEN") {
        if let Ok(v) = s.trim().parse::<u64>() {
            command_max_output_len = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_ALLOWED_COMMANDS") {
        let list: Vec<String> = s.split(',').map(|x| x.trim().to_lowercase()).filter(|x| !x.is_empty()).collect();
        if !list.is_empty() {
            allowed_commands = Some(list);
        }
    }
    if let Ok(p) = std::env::var("AGENT_RUN_COMMAND_WORKING_DIR") {
        let p = p.trim().to_string();
        if !p.is_empty() {
            run_command_working_dir = Some(p);
        }
    }
    if let Ok(s) = std::env::var("AGENT_MAX_TOKENS") {
        if let Ok(v) = s.trim().parse::<u64>() {
            max_tokens = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_TEMPERATURE") {
        if let Ok(v) = s.trim().parse::<f64>() {
            temperature = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_API_TIMEOUT_SECS") {
        if let Ok(v) = s.trim().parse::<u64>() {
            api_timeout_secs = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_API_MAX_RETRIES") {
        if let Ok(v) = s.trim().parse::<u64>() {
            api_max_retries = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_API_RETRY_DELAY_SECS") {
        if let Ok(v) = s.trim().parse::<u64>() {
            api_retry_delay_secs = Some(v);
        }
    }
    if let Ok(s) = std::env::var("AGENT_WEATHER_TIMEOUT_SECS") {
        if let Ok(v) = s.trim().parse::<u64>() {
            weather_timeout_secs = Some(v);
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

    if api_base.is_empty() {
        return Err("配置错误：未设置 api_base（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_BASE 中设置）".to_string());
    }
    if model.is_empty() {
        return Err("配置错误：未设置 model（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_MODEL 中设置）".to_string());
    }
    let max_message_history = max_message_history
        .ok_or("配置错误：未设置 max_message_history（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_MAX_MESSAGE_HISTORY 中设置）")?;
    let command_timeout_secs = command_timeout_secs
        .ok_or("配置错误：未设置 command_timeout_secs（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_COMMAND_TIMEOUT_SECS 中设置）")?;
    let command_max_output_len = command_max_output_len
        .ok_or("配置错误：未设置 command_max_output_len（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_COMMAND_MAX_OUTPUT_LEN 中设置）")?;

    if max_message_history == 0 {
        return Err("配置错误：max_message_history 必须大于 0".to_string());
    }
    if command_timeout_secs == 0 {
        return Err("配置错误：command_timeout_secs 必须大于 0".to_string());
    }
    if command_max_output_len == 0 {
        return Err("配置错误：command_max_output_len 必须大于 0".to_string());
    }
    let allowed_commands: Vec<String> = allowed_commands
        .filter(|v| !v.is_empty())
        .ok_or("配置错误：未设置 allowed_commands（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_ALLOWED_COMMANDS 中设置，逗号分隔）")?
        .into_iter()
        .map(|c| c.trim().to_lowercase())
        .filter(|c| !c.is_empty())
        .collect();
    if allowed_commands.is_empty() {
        return Err("配置错误：allowed_commands 不能为空".to_string());
    }
    let run_command_working_dir = run_command_working_dir
        .filter(|s| !s.is_empty())
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
    let run_command_working_dir = run_command_working_dir.display().to_string();

    let max_tokens = max_tokens
        .ok_or("配置错误：未设置 max_tokens（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_MAX_TOKENS 中设置）")?;
    let temperature = temperature
        .ok_or("配置错误：未设置 temperature（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_TEMPERATURE 中设置）")?;
    let api_timeout_secs = api_timeout_secs
        .ok_or("配置错误：未设置 api_timeout_secs（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_TIMEOUT_SECS 中设置）")?;
    let api_max_retries = api_max_retries
        .ok_or("配置错误：未设置 api_max_retries（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_MAX_RETRIES 中设置）")?;
    let api_retry_delay_secs = api_retry_delay_secs
        .ok_or("配置错误：未设置 api_retry_delay_secs（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_API_RETRY_DELAY_SECS 中设置）")?;
    let weather_timeout_secs = weather_timeout_secs
        .ok_or("配置错误：未设置 weather_timeout_secs（请在 default_config.toml、config.toml、.agent_demo.toml 或环境变量 AGENT_WEATHER_TIMEOUT_SECS 中设置）")?;

    if max_tokens == 0 {
        return Err("配置错误：max_tokens 必须大于 0".to_string());
    }
    if !(0.0..=2.0).contains(&temperature) {
        return Err("配置错误：temperature 应在 0～2 之间".to_string());
    }
    if api_timeout_secs == 0 {
        return Err("配置错误：api_timeout_secs 必须大于 0".to_string());
    }
    if api_max_retries > 10 {
        return Err("配置错误：api_max_retries 不宜超过 10".to_string());
    }
    if api_retry_delay_secs == 0 {
        return Err("配置错误：api_retry_delay_secs 必须大于 0".to_string());
    }
    if weather_timeout_secs == 0 {
        return Err("配置错误：weather_timeout_secs 必须大于 0".to_string());
    }

    // 系统提示词：若配置了 system_prompt_file 则从文件读取并覆盖
    if let Some(ref path) = system_prompt_file {
        if Path::new(path).exists() {
            match std::fs::read_to_string(path) {
                Ok(content) => {
                    let content = content.trim().to_string();
                    if !content.is_empty() {
                        system_prompt = content;
                    }
                }
                Err(e) => {
                    return Err(format!(
                        "配置错误：无法读取 system_prompt_file \"{}\": {}",
                        path, e
                    ));
                }
            }
        }
    }
    if system_prompt.is_empty() {
        return Err("配置错误：未设置 system_prompt（请在 default_config.toml、config.toml、.agent_demo.toml 中设置 system_prompt 或 system_prompt_file，或使用环境变量 AGENT_SYSTEM_PROMPT / AGENT_SYSTEM_PROMPT_FILE）".to_string());
    }

    Ok(AgentConfig {
        api_base,
        model,
        max_message_history: max_message_history as usize,
        command_timeout_secs,
        command_max_output_len: command_max_output_len as usize,
        allowed_commands,
        run_command_working_dir,
        max_tokens: max_tokens as u32,
        temperature: temperature as f32,
        api_timeout_secs,
        api_max_retries: api_max_retries as u32,
        api_retry_delay_secs,
        weather_timeout_secs,
        system_prompt,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_agent_section_full() {
        let s = r#"
[agent]
api_base = "https://example.com/v1"
model = "my-model"
"#;
        let agent = parse_agent_section(s).unwrap();
        assert_eq!(agent.api_base, Some("https://example.com/v1".to_string()));
        assert_eq!(agent.model, Some("my-model".to_string()));
    }

    #[test]
    fn test_parse_agent_section_empty_or_no_agent() {
        assert!(parse_agent_section("").is_none());
        assert!(parse_agent_section("[other]\nfoo = 1").is_none());
    }

    #[test]
    fn test_parse_agent_section_partial() {
        let s = r#"[agent]
model = "only-model"
"#;
        let agent = parse_agent_section(s).unwrap();
        assert_eq!(agent.api_base, None);
        assert_eq!(agent.model, Some("only-model".to_string()));
    }

    #[test]
    fn test_load_config_ok_when_default_has_all() {
        // 项目嵌入的 default_config.toml 含全部必填项，应成功
        let cfg = load_config(None).expect("default_config.toml 应提供全部配置");
        assert!(!cfg.api_base.is_empty());
        assert!(!cfg.model.is_empty());
        assert!(cfg.api_base.starts_with("http"));
        assert!(cfg.max_message_history > 0);
        assert!(cfg.command_timeout_secs > 0);
        assert!(cfg.command_max_output_len > 0);
        assert!(!cfg.allowed_commands.is_empty());
        assert!(!cfg.run_command_working_dir.is_empty());
        assert!(cfg.max_tokens > 0);
        assert!(cfg.api_timeout_secs > 0);
        assert!(cfg.api_retry_delay_secs > 0);
        assert!(cfg.weather_timeout_secs > 0);
        assert!(!cfg.system_prompt.is_empty());
    }
}
