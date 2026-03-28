//! 可选将 **`HandlerId::SyncDefault`** 工具放到 Docker 容器内执行（`sync_default_tool_sandbox_mode = docker`）。
//!
//! 通过宿主上的 **`docker` CLI**（`docker run`）启动容器，挂载工作区与宿主 `crabmate` 二进制，在容器内执行 **`crabmate tool-runner-internal`**（见 `main.rs` 早退逻辑）。
//!
//! **安全与运维**
//! - 默认 **`--network none`**（可用配置改为桥接网络名以允许联网工具）。
//! - 工作区以 **`rw`** 挂载到容器内 **`/workspace`**；工具参数中的相对路径应相对于工作区根。
//! - 临时 JSON 含 `web_search_api_key` 等字段，写入宿主临时目录；Unix 上尝试 **`0o600`**。
//! - 镜像须包含工具依赖（如 `git`、`rg`）；二进制须与宿主架构兼容（推荐同构构建或镜像内复制同版本 `crabmate`）。

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::{AgentConfig, SyncDefaultToolSandboxMode, WebSearchProvider};
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
struct ToolInvocationLine {
    tool: String,
    args_json: String,
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

fn write_runner_config_json(cfg: &AgentConfig) -> Result<PathBuf, String> {
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

/// 在 Docker 容器内执行单个 SyncDefault 工具。
pub async fn run_sync_default_in_docker(
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    tool_name: &str,
    args_json: &str,
) -> Result<String, String> {
    if cfg.sync_default_tool_sandbox_mode != SyncDefaultToolSandboxMode::Docker {
        return Err("内部错误：未启用 Docker 沙盒".to_string());
    }
    let image = cfg.sync_default_tool_sandbox_docker_image.trim();
    if image.is_empty() {
        return Err("错误：sync_default_tool_sandbox_docker_image 为空".to_string());
    }
    let exe = std::env::current_exe().map_err(|e| format!("current_exe：{}", e))?;
    let work_canon = effective_working_dir
        .canonicalize()
        .map_err(|e| format!("规范工作区路径失败：{}", e))?;
    let work_s = work_canon.to_string_lossy();
    let exe_s = exe.to_string_lossy();

    let cfg_path = write_runner_config_json(cfg)?;
    let cfg_in_container = "/run/crabmate-tool-runner.json";
    let crabmate_in_container = "/crabmate";

    let cname = format!(
        "crabmate-sd-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    );

    let network = cfg.sync_default_tool_sandbox_docker_network.trim();
    let mut docker_args: Vec<String> = vec![
        "run".into(),
        "--rm".into(),
        "-i".into(),
        "--name".into(),
        cname.clone(),
        "-w".into(),
        "/workspace".into(),
        "-v".into(),
        format!("{}:/workspace:rw", work_s),
        "-v".into(),
        format!("{}:{}:ro", exe_s, crabmate_in_container),
        "-v".into(),
        format!("{}:{}:ro", cfg_path.to_string_lossy(), cfg_in_container),
        "-e".into(),
        format!("CRABMATE_TOOL_RUNNER_CONFIG_FILE={}", cfg_in_container),
    ];
    if network.is_empty() {
        docker_args.push("--network".into());
        docker_args.push("none".into());
    } else {
        docker_args.push("--network".into());
        docker_args.push(network.to_string());
    }
    docker_args.push(image.to_string());
    docker_args.push(crabmate_in_container.to_string());
    docker_args.push("tool-runner-internal".into());

    let inv = ToolInvocationLine {
        tool: tool_name.to_string(),
        args_json: args_json.to_string(),
    };
    let line = format!(
        "{}\n",
        serde_json::to_string(&inv).map_err(|e| format!("{}", e))?
    );

    let timeout_secs = cfg.sync_default_tool_sandbox_docker_timeout_secs.max(1);
    let mut child = Command::new("docker")
        .args(&docker_args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            format!(
                "启动 docker 失败：{}（请确认已安装 docker 且当前用户可执行 docker run）",
                e
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("写入 docker stdin：{}", e))?;
    }

    let run_fut = child.wait_with_output();
    let output = tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), run_fut)
        .await
        .map_err(|_| {
            let _ = std::process::Command::new("docker")
                .args(["kill", &cname])
                .output();
            format!("Docker 沙盒执行超时（{} 秒）", timeout_secs)
        })?
        .map_err(|e| format!("等待 docker：{}", e))?;

    let _ = std::fs::remove_file(&cfg_path);

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Docker 沙盒失败（退出码 {:?}）：{}",
            output.status.code(),
            err.trim()
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("工具输出非 UTF-8：{}", e))
}
