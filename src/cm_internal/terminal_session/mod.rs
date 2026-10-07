//! Linux PTY 交互会话工具 **`terminal_session`**（其它平台返回明确错误）。
//!
//! 主执行路径在 [`crate::cm_internal::tool_registry`] 的异步分发中，以便下发 SSE **`tool_output_chunk`**。

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
mod pty_io;
#[cfg(target_os = "linux")]
mod sse_chunk;

#[cfg(target_os = "linux")]
pub use linux::execute_terminal_session;

/// SSE 下发通道三件套（输出通道 / 控制镜像 / 编码器）：打包以收敛 `terminal_session` 形参。
pub struct TerminalSseSink<'a> {
    pub out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    pub control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    pub encoder: Option<&'a dyn crate::cm_sse_protocol::sse::SseEncoder>,
}

#[cfg(not(target_os = "linux"))]
pub async fn execute_terminal_session(
    _cfg: &std::sync::Arc<crate::cm_config::AgentConfig>,
    _workspace: &std::path::Path,
    _args_json: &str,
    _tool_call_id: &str,
    _sse: TerminalSseSink<'_>,
    _allowed_commands: &[String],
    _skip_arg_safety: bool,
    _cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
) -> String {
    "错误：terminal_session 仅支持 Linux。".to_string()
}
