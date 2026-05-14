//! 使用 [bollard](https://docs.rs/bollard) 通过 **Docker Engine HTTP API** 执行 [`super::backend::SandboxRunRequest`]。
//!
//! - **Unix**：[`Docker::connect_with_local_defaults`]（默认 `/var/run/docker.sock` 或 `DOCKER_HOST` 中的 unix://）。
//! - **非 Unix**：[`Docker::connect_with_defaults`]（`DOCKER_HOST`）。

use bollard::Docker;
use bollard::container::{
    AttachContainerOptions, Config, CreateContainerOptions, LogOutput, RemoveContainerOptions,
    StartContainerOptions,
};
use bollard::models::HostConfig;
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;

use super::backend::{SandboxRunRequest, SyncDefaultSandboxBackend};

/// 通过 Docker Engine API 执行隔离运行（见模块说明）。
#[derive(Debug, Default, Clone, Copy)]
pub struct BollardSandboxBackend;

#[async_trait::async_trait]
impl SyncDefaultSandboxBackend for BollardSandboxBackend {
    async fn run_isolated(&self, req: SandboxRunRequest) -> Result<Vec<u8>, String> {
        run_isolated_bollard(req).await
    }
}

async fn bollard_connect_docker() -> Result<Docker, String> {
    #[cfg(unix)]
    {
        Docker::connect_with_local_defaults()
            .map_err(|e| format!("连接 Docker Engine（bollard Unix 套接字）：{}", e))
    }
    #[cfg(not(unix))]
    {
        Docker::connect_with_defaults()
            .map_err(|e| format!("连接 Docker Engine（bollard，见 DOCKER_HOST）：{}", e))
    }
}

fn bollard_ephemeral_container_name() -> Result<String, String> {
    Ok(format!(
        "crabmate-sd-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos()
    ))
}

fn bollard_network_mode(req: &SandboxRunRequest) -> String {
    req.network_mode
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "none".to_string())
}

fn bollard_container_config(req: &SandboxRunRequest, network_mode: String) -> Config<String> {
    let host_config = HostConfig {
        binds: Some(req.binds.clone()),
        network_mode: Some(network_mode),
        ..Default::default()
    };
    Config {
        image: Some(req.image.clone()),
        cmd: Some(req.cmd.clone()),
        env: Some(req.env.clone()),
        working_dir: Some(req.working_dir.clone()),
        user: req.user.clone(),
        host_config: Some(host_config),
        attach_stdin: Some(true),
        attach_stdout: Some(true),
        attach_stderr: Some(true),
        open_stdin: Some(true),
        tty: Some(false),
        ..Default::default()
    }
}

async fn bollard_run_attached_and_wait_stdout(
    docker: &Docker,
    container_id: &str,
    stdin_payload: &[u8],
) -> Result<Vec<u8>, String> {
    docker
        .start_container(container_id, None::<StartContainerOptions<String>>)
        .await
        .map_err(|e| format!("docker start_container：{}", e))?;

    let attach_opts = AttachContainerOptions::<String> {
        stdin: Some(true),
        stdout: Some(true),
        stderr: Some(true),
        stream: Some(true),
        ..Default::default()
    };

    let mut attach = docker
        .attach_container(container_id, Some(attach_opts))
        .await
        .map_err(|e| format!("docker attach_container：{}", e))?;

    let mut input = attach.input;
    input
        .as_mut()
        .write_all(stdin_payload)
        .await
        .map_err(|e| format!("写入容器 stdin：{}", e))?;
    input
        .as_mut()
        .shutdown()
        .await
        .map_err(|e| format!("关闭容器 stdin：{}", e))?;

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    while let Some(item) = attach.output.next().await {
        let item = item.map_err(|e| format!("attach 输出流：{}", e))?;
        match item {
            LogOutput::StdOut { message } => stdout.extend_from_slice(&message),
            LogOutput::StdErr { message } => stderr.extend_from_slice(&message),
            LogOutput::Console { message } => stdout.extend_from_slice(&message),
            LogOutput::StdIn { message: _ } => {}
        }
    }

    let mut wait_stream = docker.wait_container::<String>(container_id, None);
    let wait_item = wait_stream
        .next()
        .await
        .transpose()
        .map_err(|e| format!("docker wait_container：{}", e))?;
    let code = wait_item.map(|w| w.status_code).unwrap_or(-1);

    if code != 0 {
        let err = String::from_utf8_lossy(&stderr);
        return Err(format!("沙盒内进程退出码 {}：{}", code, err.trim()));
    }

    Ok(stdout)
}

async fn run_isolated_bollard(req: SandboxRunRequest) -> Result<Vec<u8>, String> {
    let docker = bollard_connect_docker().await?;
    let name = bollard_ephemeral_container_name()?;
    let network_mode = bollard_network_mode(&req);
    let config = bollard_container_config(&req, network_mode);

    let create = CreateContainerOptions {
        name: name.clone(),
        platform: None,
    };

    let res = docker
        .create_container(Some(create), config)
        .await
        .map_err(|e| format!("docker create_container：{}", e))?;
    let id = res.id;

    let remove_opts = RemoveContainerOptions {
        force: true,
        ..Default::default()
    };

    let run_inner = bollard_run_attached_and_wait_stdout(&docker, &id, &req.stdin_payload);

    let outcome = tokio::time::timeout(req.timeout, run_inner).await;
    let _ = docker.remove_container(&id, Some(remove_opts)).await;

    match outcome {
        Ok(inner) => inner,
        Err(_) => Err(format!(
            "Docker Engine 沙盒超时（{} 秒）",
            req.timeout.as_secs()
        )),
    }
}
