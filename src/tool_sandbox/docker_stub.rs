//! `docker_sandbox` feature 关闭时：占位后端，拒绝容器内执行。

use crate::tool_sandbox::backend::{SandboxRunRequest, SyncDefaultSandboxBackend};

/// 与 `docker_bollard::BollardSandboxBackend` 同名的占位类型（未链接 **bollard** 时使用）。
pub struct BollardSandboxBackend;

#[async_trait::async_trait]
impl SyncDefaultSandboxBackend for BollardSandboxBackend {
    async fn run_isolated(&self, _req: SandboxRunRequest) -> Result<Vec<u8>, String> {
        Err(
            "本 crabmate 二进制未启用 `docker_sandbox` Cargo feature，无法连接 Docker Engine"
                .to_string(),
        )
    }
}
