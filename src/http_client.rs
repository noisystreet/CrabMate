//! 对外部模型 API（及同类 HTTPS 端点）的 **`reqwest::Client` 单例构造**。
//!
//! ## 与「长连接」的关系
//!
//! OpenAI/DeepSeek 兼容接口是 **HTTP**（JSON 或 **SSE 流**），不是单条 WebSocket 会话。所谓「长连接」在这里指：
//!
//! - **HTTP Keep-Alive**：同一 `Client` 多次 `POST /chat/completions` 时，在连接池内**复用 TCP/TLS**，减少握手。
//! - **连接池**：`pool_max_idle_per_host` / `pool_idle_timeout` 控制每主机空闲连接数与保留时间。
//! - **TCP keepalive**：降低 NAT/防火墙对空闲连接的提前断开概率（两次请求间隔较长时仍可能复用）。
//!
//! 进程内应对 **`api_base` 指向的模型服务** 只使用**一个**共享 `Client`（见 `run()` 中的 `AppState`），勿每请求 `Client::new()`。

use std::error::Error as StdError;
use std::time::Duration;

use reqwest::Client;

use crate::config::AgentConfig;

/// 将 `reqwest` 传输错误格式化为可读说明（日志与 CLI），不输出密钥；附带超时/连接类提示便于排障。
///
/// [`map_reqwest_transport_err`] 与 LLM 层 [`crate::llm::call_error::LlmCallError`] 共用此文案。
pub fn format_reqwest_transport_err(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    if e.is_timeout() {
        msg.push_str(
            " [提示：连接或整请求超时，可调大配置 [agent] api_timeout_secs，或检查网络/代理]",
        );
    } else if e.is_connect() {
        msg.push_str(" [提示：无法建立 TLS/TCP 连接，常见于 DNS 失败、防火墙、需代理（HTTPS_PROXY）、或对端不可达；可用 curl -v 测同一 URL]");
    }
    if let Some(src) = e.source() {
        msg.push_str(" | ");
        msg.push_str(&src.to_string());
    }
    msg
}

/// 将 `reqwest` 传输错误转为可读说明（日志与 CLI），不输出密钥；附带超时/连接类提示便于排障。
pub fn map_reqwest_transport_err(e: reqwest::Error) -> Box<dyn std::error::Error + Send + Sync> {
    std::io::Error::other(format_reqwest_transport_err(&e)).into()
}

/// 建立 TLS 等阶段的上限，避免坏网络长时间挂死（与整请求 `timeout` 区分）。
fn connect_timeout_for(cfg: &AgentConfig) -> Duration {
    let secs = cfg.api_timeout_secs.clamp(5, 45);
    Duration::from_secs(secs)
}

/// 构造供全进程复用的异步 HTTP 客户端（连接池 + Keep-Alive 友好设置）。
pub fn build_shared_api_client(cfg: &AgentConfig) -> Result<Client, reqwest::Error> {
    Client::builder()
        // 整次请求（含读完全部响应体；流式直到 `[DONE]`）的上限
        .timeout(Duration::from_secs(cfg.api_timeout_secs))
        .connect_timeout(connect_timeout_for(cfg))
        // 对单一模型网关多轮对话/工具循环时复用连接
        .pool_max_idle_per_host(8)
        .pool_idle_timeout(Duration::from_secs(120))
        .tcp_keepalive(Duration::from_secs(60))
        .user_agent(concat!("crabmate/", env!("CARGO_PKG_VERSION")))
        .build()
}
