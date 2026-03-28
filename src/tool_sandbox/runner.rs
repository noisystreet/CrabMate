//! 容器内 `tool-runner-internal` 与宿主侧临时配置 JSON。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::config::{AgentConfig, WebSearchProvider};
use crate::tools::{ToolContext, run_tool};

/// 由容器内 `crabmate tool-runner-internal` 读取（`CRABMATE_TOOL_RUNNER_CONFIG_FILE`）。
#[derive(Debug, Serialize, Deserialize)]
pub struct SandboxToolRunnerConfig {
    pub command_max_output_len: usize,
    pub weather_timeout_secs: u64,
    pub allowed_commands: Vec<String>,
    pub web_search_provider: String,
    pub web_search_api_key: String,
    pub web_search_timeout_secs: u64,
    pub web_search_max_results: u32,
    pub http_fetch_allowed_prefixes: Vec<String>,
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
}

impl SandboxToolRunnerConfig {
    pub fn from_agent_config(cfg: &AgentConfig) -> Self {
        Self {
            command_max_output_len: cfg.command_max_output_len,
            weather_timeout_secs: cfg.weather_timeout_secs,
            allowed_commands: cfg.allowed_commands.iter().cloned().collect(),
            web_search_provider: cfg.web_search_provider.as_str().to_string(),
            web_search_api_key: cfg.web_search_api_key.clone(),
            web_search_timeout_secs: cfg.web_search_timeout_secs,
            web_search_max_results: cfg.web_search_max_results,
            http_fetch_allowed_prefixes: cfg.http_fetch_allowed_prefixes.clone(),
            http_fetch_timeout_secs: cfg.http_fetch_timeout_secs,
            http_fetch_max_response_bytes: cfg.http_fetch_max_response_bytes,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolInvocationLine {
    pub tool: String,
    pub args_json: String,
}

/// 容器内入口：读 `CRABMATE_TOOL_RUNNER_CONFIG_FILE`，从 stdin 读一行 JSON，向 stdout 打印工具输出。
pub fn tool_runner_internal_main() -> Result<(), String> {
    let path = std::env::var("CRABMATE_TOOL_RUNNER_CONFIG_FILE")
        .map_err(|_| "缺少环境变量 CRABMATE_TOOL_RUNNER_CONFIG_FILE".to_string())?;
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("读取工具运行器配置失败：{} ({})", e, path))?;
    let snap: SandboxToolRunnerConfig =
        serde_json::from_str(&raw).map_err(|e| format!("解析工具运行器配置失败：{}", e))?;
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("读取 stdin 失败：{}", e))?;
    let inv: ToolInvocationLine = serde_json::from_str(line.trim()).map_err(|e| {
        format!(
            "解析工具调用行失败（须为一行 JSON：{{\"tool\":\"…\",\"args_json\":\"…\"}}）：{}",
            e
        )
    })?;
    let allowed = Box::leak(snap.allowed_commands.into_boxed_slice());
    let prefixes = Box::leak(snap.http_fetch_allowed_prefixes.into_boxed_slice());
    let key = Box::leak(snap.web_search_api_key.into_boxed_str());
    let provider =
        WebSearchProvider::parse(&snap.web_search_provider).map_err(|e| e.to_string())?;
    let ctx = ToolContext {
        command_max_output_len: snap.command_max_output_len,
        weather_timeout_secs: snap.weather_timeout_secs,
        allowed_commands: allowed,
        working_dir: Path::new("/workspace"),
        web_search_timeout_secs: snap.web_search_timeout_secs,
        web_search_provider: provider,
        web_search_api_key: key,
        web_search_max_results: snap.web_search_max_results,
        http_fetch_allowed_prefixes: prefixes,
        http_fetch_timeout_secs: snap.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: snap.http_fetch_max_response_bytes,
        read_file_turn_cache: None,
    };
    let out = run_tool(&inv.tool, &inv.args_json, &ctx);
    print!("{out}");
    Ok(())
}

pub fn write_runner_config_json(cfg: &AgentConfig) -> Result<PathBuf, String> {
    let snap = SandboxToolRunnerConfig::from_agent_config(cfg);
    let json = serde_json::to_string(&snap).map_err(|e| format!("序列化沙盒配置：{}", e))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = std::env::temp_dir().join(format!("crabmate-tool-runner-{nanos}.json"));
    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("创建临时配置：{}", e))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("写入临时配置：{}", e))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&path, json).map_err(|e| format!("写入临时配置：{}", e))?;
    }
    Ok(path)
}
