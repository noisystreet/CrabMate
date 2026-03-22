//! 命令行与终端 UI 运行时，以及二者与 `api` 共用的终端侧能力（`latex_unicode`）、会话文件/导出（`chat_export`）。
pub mod chat_export;
pub mod cli;
pub mod latex_unicode;
pub(crate) mod terminal_labels;
pub mod tui;
