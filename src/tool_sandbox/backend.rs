//! SyncDefault 沙盒的**容器后端**抽象：便于替换为 bollard、其它 OCI 运行时或测试替身。

use std::time::Duration;

/// 单次工具调用在沙盒内执行所需的参数（与 Docker / OCI 通用字段对齐）。
#[derive(Debug, Clone)]
pub struct SandboxRunRequest {
    /// 容器镜像（如 `registry/crabmate-tools:tag`）。
    pub image: String,
    /// `None` 表示隔离网络（Docker 下对应 `network_mode: none`）；`Some("bridge")` 等表示命名网络。
    pub network_mode: Option<String>,
    /// `docker` 风格 bind：`/host/path:/container/path:ro|rw`。
    pub binds: Vec<String>,
    /// `KEY=value` 环境变量。
    pub env: Vec<String>,
    /// 容器工作目录。
    pub working_dir: String,
    /// 入口命令 argv（不含 `image`）。
    pub cmd: Vec<String>,
    /// 写入容器 stdin 的字节（UTF-8 文本；通常为单行 JSON + `\n`）。
    pub stdin_payload: Vec<u8>,
    /// 整体超时（创建 → attach → 等待退出）。
    pub timeout: Duration,
}

/// 在隔离环境中执行一次 `SandboxRunRequest`，返回容器**标准输出**字节（工具原文）。
#[async_trait::async_trait]
pub trait SyncDefaultSandboxBackend: Send + Sync {
    async fn run_isolated(&self, req: SandboxRunRequest) -> Result<Vec<u8>, String>;
}
