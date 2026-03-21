//! 运行配置：API 地址、模型等，从 default_config.toml + 可选覆盖

pub mod cli;

use crate::per_coord::FinalPlanRequirementMode;
use serde::Deserialize;
use std::path::Path;

/// 编译时嵌入的默认配置（与项目根 default_config.toml 一致）
const DEFAULT_CONFIG: &str = include_str!("../../default_config.toml");

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
    /// workflow 反思：模型未在 `workflow.reflection.max_rounds` 中指定时的默认上限（传给 `WorkflowReflectionController` / `PerCoordinator`）
    pub reflection_default_max_rounds: usize,
    /// 何时强制终答含 `agent_reply_plan` v1（见 `per_coord::FinalPlanRequirementMode`）
    pub final_plan_requirement: FinalPlanRequirementMode,
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
    reflection_default_max_rounds: Option<u64>,
    /// `never` / `workflow_reflection` / `always`
    final_plan_requirement: Option<String>,
    system_prompt: Option<String>,
    system_prompt_file: Option<String>,
    env: Option<String>,
    allowed_commands_dev: Option<Vec<String>>,
    allowed_commands_prod: Option<Vec<String>>,
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
    let mut reflection_default_max_rounds: Option<u64> = None;
    let mut final_plan_requirement_str: Option<String> = None;
    let mut allowed_commands: Option<Vec<String>> = None;
    let mut allowed_commands_dev: Option<Vec<String>> = None;
    let mut allowed_commands_prod: Option<Vec<String>> = None;
    let mut run_command_working_dir: Option<String> = None;
    let mut env_tag: Option<String> = None;

    if let Some(agent) = parse_agent_section(DEFAULT_CONFIG) {
        api_base = agent.api_base.unwrap_or_default().trim().to_string();
        model = agent.model.unwrap_or_default().trim().to_string();
        max_message_history = agent.max_message_history.or(max_message_history);
        command_timeout_secs = agent.command_timeout_secs.or(command_timeout_secs);
        command_max_output_len = agent.command_max_output_len.or(command_max_output_len);
        if let Some(ref v) = agent.allowed_commands
            && !v.is_empty() {
                allowed_commands = Some(v.clone());
            }
        if let Some(ref v) = agent.allowed_commands_dev
            && !v.is_empty() {
                allowed_commands_dev = Some(v.clone());
            }
        if let Some(ref v) = agent.allowed_commands_prod
            && !v.is_empty() {
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
        reflection_default_max_rounds = agent
            .reflection_default_max_rounds
            .or(reflection_default_max_rounds);
        if let Some(ref s) = agent.final_plan_requirement {
            let s = s.trim().to_string();
            if !s.is_empty() {
                final_plan_requirement_str = Some(s);
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
        if let Some(e) = agent.env {
            let e = e.trim().to_string();
            if !e.is_empty() {
                env_tag = Some(e);
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
                if let Some(ref v) = agent.allowed_commands
                    && !v.is_empty() {
                        allowed_commands = Some(v.clone());
                    }
                if let Some(ref v) = agent.allowed_commands_dev
                    && !v.is_empty() {
                        allowed_commands_dev = Some(v.clone());
                    }
                if let Some(ref v) = agent.allowed_commands_prod
                    && !v.is_empty() {
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
                if let Some(v) = agent.reflection_default_max_rounds {
                    reflection_default_max_rounds = Some(v);
                }
                if let Some(ref s) = agent.final_plan_requirement {
                    let s = s.trim().to_string();
                    if !s.is_empty() {
                        final_plan_requirement_str = Some(s);
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
                if let Some(e) = agent.env {
                    let e = e.trim().to_string();
                    if !e.is_empty() {
                        env_tag = Some(e);
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
    if let Ok(v) = std::env::var("AGENT_MAX_MESSAGE_HISTORY")
        && let Ok(n) = v.trim().parse::<u64>() {
            max_message_history = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_COMMAND_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>() {
            command_timeout_secs = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_COMMAND_MAX_OUTPUT_LEN")
        && let Ok(n) = v.trim().parse::<u64>() {
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
    if let Ok(v) = std::env::var("AGENT_MAX_TOKENS")
        && let Ok(n) = v.trim().parse::<u64>() {
            max_tokens = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_TEMPERATURE")
        && let Ok(n) = v.trim().parse::<f64>() {
            temperature = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_API_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>() {
            api_timeout_secs = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_API_MAX_RETRIES")
        && let Ok(n) = v.trim().parse::<u64>() {
            api_max_retries = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_API_RETRY_DELAY_SECS")
        && let Ok(n) = v.trim().parse::<u64>() {
            api_retry_delay_secs = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_WEATHER_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>() {
            weather_timeout_secs = Some(n);
        }
    if let Ok(v) = std::env::var("AGENT_REFLECTION_DEFAULT_MAX_ROUNDS")
        && let Ok(n) = v.trim().parse::<u64>() {
            reflection_default_max_rounds = Some(n);
        }
    if let Ok(s) = std::env::var("AGENT_FINAL_PLAN_REQUIREMENT") {
        let s = s.trim().to_string();
        if !s.is_empty() {
            final_plan_requirement_str = Some(s);
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
        .unwrap_or(32)
        .clamp(1, 1024) as usize;
    let command_timeout_secs = command_timeout_secs.unwrap_or(30).max(1);
    let command_max_output_len = command_max_output_len.unwrap_or(8192).clamp(1024, 131072) as usize;
    let max_tokens = max_tokens.unwrap_or(4096).clamp(256, 32768) as u32;
    let temperature = temperature.unwrap_or(0.3).clamp(0.0, 2.0) as f32;
    let api_timeout_secs = api_timeout_secs.unwrap_or(60).max(1);
    let api_max_retries = api_max_retries.unwrap_or(2).min(10) as u32;
    let api_retry_delay_secs = api_retry_delay_secs.unwrap_or(2).max(1);
    let weather_timeout_secs = weather_timeout_secs.unwrap_or(15).max(1);
    let reflection_default_max_rounds =
        reflection_default_max_rounds.unwrap_or(5).max(1) as usize;

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
            "gcc".into(),
            "g++".into(),
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

    let system_prompt = if let Some(path) = system_prompt_file {
        let path = Path::new(&path);
        
        std::fs::read_to_string(path).map_err(|e| {
            format!("无法读取 system_prompt_file \"{}\": {}", path.display(), e)
        })?
    } else {
        system_prompt
    };
    if system_prompt.trim().is_empty() {
        return Err("配置错误：未设置 system_prompt 或 system_prompt_file".to_string());
    }

    let final_plan_requirement = match final_plan_requirement_str.as_deref() {
        Some(s) => FinalPlanRequirementMode::parse(s)?,
        None => FinalPlanRequirementMode::default(),
    };

    Ok(AgentConfig {
        api_base,
        model,
        max_message_history,
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
        reflection_default_max_rounds,
        final_plan_requirement,
        system_prompt,
    })
}

