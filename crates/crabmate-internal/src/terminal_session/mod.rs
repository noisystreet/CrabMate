//! Linux PTY 交互会话工具 **`terminal_session`**（其它平台返回明确错误）。
//!
//! 主执行路径在 [`crate::tool_registry`] 的异步分发中，以便下发 SSE **`tool_output_chunk`**。

#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::execute_terminal_session;

#[cfg(not(target_os = "linux"))]
pub async fn execute_terminal_session(
    _cfg: &std::sync::Arc<crabmate_config::AgentConfig>,
    _workspace: &std::path::Path,
    _args_json: &str,
    _tool_call_id: &str,
    _sse_out_tx: Option<&tokio::sync::mpsc::Sender<String>>,
    _sse_control_mirror: Option<&crabmate_sse_protocol::sse::SseControlMirror>,
    _allowed_commands: &[String],
    _encoder: Option<&dyn crabmate_sse_protocol::sse::SseEncoder>,
) -> String {
    "错误：terminal_session 仅支持 Linux。".to_string()
}
