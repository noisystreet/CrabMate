//! Docker 沙盒（`sync_default_tool_sandbox_mode = docker`）：在隔离容器中执行**多类**工具（见 `ToolInvocationLine.kind`）。
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
pub use runner::{
    SandboxToolRunnerConfig, ToolInvocationLine, tool_runner_internal_main,
    write_runner_config_json, write_runner_config_json_with_allowed_commands,
};

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::{AgentConfig, SyncDefaultToolSandboxMode};

use self::runner::write_runner_config_json as write_runner_cfg_default;

/// 进程内默认后端（Docker Engine API，[bollard](https://docs.rs/bollard)）。
///
/// 若需替换实现（单测或其它运行时），可改为 `OnceLock<Arc<dyn SyncDefaultSandboxBackend>>` 并在启动时注入。
static SANDBOX_BACKEND: std::sync::LazyLock<std::sync::Arc<dyn SyncDefaultSandboxBackend>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(BollardSandboxBackend));

/// 是否启用 Docker 沙盒（与 `dispatch_tool` 中多 handler 共用）。
pub fn docker_sandbox_enabled(cfg: &AgentConfig) -> bool {
    cfg.sync_default_tool_sandbox_mode == SyncDefaultToolSandboxMode::Docker
}

/// 在沙盒内执行一次工具（经 [`SANDBOX_BACKEND`]）；`cfg_json_path` 由调用方写入后传入，本函数结束时删除。
pub async fn run_tool_in_docker(
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    cfg_json_path: PathBuf,
    inv: ToolInvocationLine,
) -> Result<String, String> {
    if !docker_sandbox_enabled(cfg) {
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
        format!(
            "{}:{}:ro",
            cfg_json_path.to_string_lossy(),
            cfg_in_container
        ),
    ];

    let env = vec![format!(
        "CRABMATE_TOOL_RUNNER_CONFIG_FILE={}",
        cfg_in_container
    )];

    let cmd = vec![
        crabmate_in_container.to_string(),
        "tool-runner-internal".to_string(),
    ];

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
    let _ = std::fs::remove_file(&cfg_json_path);
    let bytes = out?;
    String::from_utf8(bytes).map_err(|e| format!("工具输出非 UTF-8：{}", e))
}

/// 在沙盒内执行单个 `SyncDefault` 工具。
pub async fn run_sync_default_in_docker(
    cfg: &AgentConfig,
    effective_working_dir: &Path,
    tool_name: &str,
    args_json: &str,
) -> Result<String, String> {
    let cfg_path = write_runner_cfg_default(cfg)?;
    let inv = ToolInvocationLine {
        kind: "sync_default".to_string(),
        tool: Some(tool_name.to_string()),
        args_json: args_json.to_string(),
    };
    run_tool_in_docker(cfg, effective_working_dir, cfg_path, inv).await
}
