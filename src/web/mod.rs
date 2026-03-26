//! Axum handler：浏览器 Web UI 调用的工作区、任务等 HTTP API（**非**终端 TUI；TUI 在 `runtime/tui`）。
mod app_state;
mod chat_handlers;

pub(crate) use app_state::{AppState, SaveConversationOutcome};
pub(crate) use chat_handlers::{cleanup_uploads_dir, save_outcome_to_stream_error_line};

#[cfg(test)]
pub(crate) use chat_handlers::normalize_client_conversation_id;

pub mod server;
pub mod task;
pub mod workspace;
