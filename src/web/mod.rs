//! Axum handler：浏览器 Web UI 调用的工作区、任务等 HTTP API（**非**终端 TUI；TUI 在 `runtime/tui`）。
pub mod server;
pub mod task;
pub mod workspace;
