//! 容器内 `tool-runner-internal` 与宿主侧临时配置 JSON。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::codebase_semantic_index::CodebaseSemanticToolParams;
use crate::config::{AgentConfig, ExposeSecret, WebSearchProvider};
use crate::tools::http_fetch;
use crate::tools::{ToolContext, run_tool};

fn default_test_result_cache_enabled() -> bool {
    true
}

fn default_test_result_cache_max_entries() -> usize {
    32
}

fn default_command_timeout_secs() -> u64 {
    30
}

/// 由容器内 `crabmate tool-runner-internal` 读取（`CRABMATE_TOOL_RUNNER_CONFIG_FILE`）。
#[derive(Debug, Serialize, Deserialize)]
pub struct SandboxToolRunnerConfig {
    pub command_max_output_len: usize,
    pub weather_timeout_secs: u64,
    #[serde(default = "default_command_timeout_secs")]
    pub command_timeout_secs: u64,
    #[serde(default = "default_test_result_cache_enabled")]
    pub test_result_cache_enabled: bool,
    #[serde(default = "default_test_result_cache_max_entries")]
    pub test_result_cache_max_entries: usize,
    pub allowed_commands: Vec<String>,
    pub web_search_provider: String,
    pub web_search_api_key: String,
    pub web_search_timeout_secs: u64,
    pub web_search_max_results: u32,
    pub http_fetch_allowed_prefixes: Vec<String>,
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
    pub codebase_semantic: CodebaseSemanticToolParams,
}

impl SandboxToolRunnerConfig {
    pub fn from_agent_config(cfg: &AgentConfig) -> Self {
        Self {
            command_max_output_len: cfg.command_max_output_len,
            weather_timeout_secs: cfg.weather_timeout_secs,
            command_timeout_secs: cfg.command_timeout_secs,
            test_result_cache_enabled: cfg.test_result_cache_enabled,
            test_result_cache_max_entries: cfg.test_result_cache_max_entries,
            allowed_commands: cfg.allowed_commands.iter().cloned().collect(),
            web_search_provider: cfg.web_search_provider.as_str().to_string(),
            web_search_api_key: cfg.web_search_api_key.expose_secret().to_string(),
            web_search_timeout_secs: cfg.web_search_timeout_secs,
            web_search_max_results: cfg.web_search_max_results,
            http_fetch_allowed_prefixes: cfg.http_fetch_allowed_prefixes.clone(),
            http_fetch_timeout_secs: cfg.http_fetch_timeout_secs,
            http_fetch_max_response_bytes: cfg.http_fetch_max_response_bytes,
            codebase_semantic: CodebaseSemanticToolParams::from_agent_config(cfg),
        }
    }

    /// `run_command` 经审批扩展白名单后，须把**有效**列表写入沙盒 JSON（容器内 `run_tool` 用）。
    pub fn from_agent_config_with_allowed_commands(cfg: &AgentConfig, allowed: &[String]) -> Self {
        let mut s = Self::from_agent_config(cfg);
        s.allowed_commands = allowed.to_vec();
        s
    }
}

fn default_inv_kind() -> String {
    "sync_default".to_string()
}

/// stdin 一行 JSON：`kind` 省略时等价于 `sync_default`（兼容旧载荷）。
#[derive(Debug, Serialize, Deserialize)]
pub struct ToolInvocationLine {
    #[serde(default = "default_inv_kind")]
    pub kind: String,
    /// `sync_default` 必填；其它 kind 忽略。
    #[serde(default)]
    pub tool: Option<String>,
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
            "解析工具调用行失败（须为一行 JSON，含 kind 与 args_json）：{}",
            e
        )
    })?;
    let allowed = Box::leak(snap.allowed_commands.into_boxed_slice());
    let prefixes = Box::leak(snap.http_fetch_allowed_prefixes.into_boxed_slice());
    let key = Box::leak(snap.web_search_api_key.into_boxed_str());
    let provider =
        WebSearchProvider::parse(&snap.web_search_provider).map_err(|e| e.to_string())?;
    let ctx = ToolContext {
        cfg: None,
        codebase_semantic: Some(snap.codebase_semantic),
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
        command_timeout_secs: snap.command_timeout_secs,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: snap.test_result_cache_enabled,
        test_result_cache_max_entries: snap.test_result_cache_max_entries,
        long_term_memory: None,
        long_term_memory_scope_id: None,
    };
    let k = inv.kind.trim();
    let out = match k {
        "sync_default" => {
            let tool = inv
                .tool
                .as_deref()
                .filter(|s| !s.is_empty())
                .ok_or_else(|| "sync_default 须提供非空 tool".to_string())?;
            run_tool(tool, &inv.args_json, &ctx)
        }
        "get_weather" => run_tool("get_weather", &inv.args_json, &ctx),
        "web_search" => run_tool("web_search", &inv.args_json, &ctx),
        "run_command" => run_tool("run_command", &inv.args_json, &ctx),
        "run_executable" => run_tool("run_executable", &inv.args_json, &ctx),
        "http_fetch" => match http_fetch::parse_http_fetch_args(&inv.args_json) {
            Ok((u, m)) => http_fetch::fetch_with_method(
                &u,
                m,
                snap.http_fetch_timeout_secs.max(1),
                snap.http_fetch_max_response_bytes,
            ),
            Err(e) => format!("错误：{}", e),
        },
        "http_request" => match http_fetch::parse_http_request_args(&inv.args_json) {
            Ok((u, m, b)) => http_fetch::request_with_json_body(
                &u,
                m,
                b.as_ref(),
                snap.http_fetch_timeout_secs.max(1),
                snap.http_fetch_max_response_bytes,
            ),
            Err(e) => format!("错误：{}", e),
        },
        _ => {
            return Err(format!(
                "未知的沙盒调用 kind: {:?}（支持 sync_default、get_weather、web_search、http_fetch、http_request、run_command、run_executable）",
                inv.kind
            ));
        }
    };
    print!("{out}");
    Ok(())
}

pub fn write_runner_config_json(cfg: &AgentConfig) -> Result<PathBuf, String> {
    write_runner_config_json_inner(SandboxToolRunnerConfig::from_agent_config(cfg))
}

pub fn write_runner_config_json_with_allowed_commands(
    cfg: &AgentConfig,
    allowed: &[String],
) -> Result<PathBuf, String> {
    write_runner_config_json_inner(
        SandboxToolRunnerConfig::from_agent_config_with_allowed_commands(cfg, allowed),
    )
}

fn write_runner_config_json_inner(snap: SandboxToolRunnerConfig) -> Result<PathBuf, String> {
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
