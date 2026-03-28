//! **`HandlerId::SyncDefault`** 可选沙盒：通过可插拔后端在隔离环境中执行工具。
//!
//! 默认后端为 **[bollard](https://docs.rs/bollard)**（Docker Engine HTTP API，本地套接字或 `DOCKER_HOST`）。
//!
//! **安全与运维**（与实现无关的约束）
//! - 工作区挂载到容器内 **`/workspace`**（rw）；相对路径以工作区根为基准。
//! - 临时 JSON 含 `web_search_api_key` 等；Unix 上尝试 **`0o600`**。
//! - 镜像须含工具依赖；宿主 `crabmate` 与容器须**同 CPU 架构**（或改在镜像内安装 crabmate）。

mod backend;
mod docker_bollard;
mod runner;

pub use backend::{SandboxRunRequest, SyncDefaultSandboxBackend};
pub use docker_bollard::BollardSandboxBackend;
pub use runner::{SandboxToolRunnerConfig, tool_runner_internal_main};

use std::path::Path;
use std::time::Duration;

use crate::config::{AgentConfig, SyncDefaultToolSandboxMode};

use self::runner::{ToolInvocationLine, write_runner_config_json};

/// 进程内默认后端（Docker Engine API，[bollard](https://docs.rs/bollard)）。
///
/// 若需替换实现（单测或其它运行时），可改为 `OnceLock<Arc<dyn SyncDefaultSandboxBackend>>` 并在启动时注入。
static SANDBOX_BACKEND: std::sync::LazyLock<std::sync::Arc<dyn SyncDefaultSandboxBackend>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(BollardSandboxBackend));

/// 在沙盒内执行单个 SyncDefault 工具（经 [`SANDBOX_BACKEND`]）。
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

    let network = cfg.sync_default_tool_sandbox_docker_network.trim();
    let network_mode = if network.is_empty() {
        None
    } else {
        Some(network.to_string())
    };

    let binds = vec![
        format!("{}:/workspace:rw", work_s),
        format!("{}:{}:ro", exe_s, crabmate_in_container),
        format!("{}:{}:ro", cfg_path.to_string_lossy(), cfg_in_container),
    ];

    let env = vec![format!(
        "CRABMATE_TOOL_RUNNER_CONFIG_FILE={}",
        cfg_in_container
    )];

    let cmd = vec![
        crabmate_in_container.to_string(),
        "tool-runner-internal".to_string(),
    ];

    let inv = ToolInvocationLine {
        tool: tool_name.to_string(),
        args_json: args_json.to_string(),
    };
    let stdin_payload = format!(
        "{}\n",
        serde_json::to_string(&inv).map_err(|e| format!("{}", e))?
    )
    .into_bytes();

    let timeout_secs = cfg.sync_default_tool_sandbox_docker_timeout_secs.max(1);
    let req = SandboxRunRequest {
        image: image.to_string(),
        network_mode,
        binds,
        env,
        working_dir: "/workspace".to_string(),
        cmd,
        stdin_payload,
        timeout: Duration::from_secs(timeout_secs),
    };

    let out = SANDBOX_BACKEND.run_isolated(req).await;
    let _ = std::fs::remove_file(&cfg_path);
    let bytes = out?;
    String::from_utf8(bytes).map_err(|e| format!("工具输出非 UTF-8：{}", e))
}
