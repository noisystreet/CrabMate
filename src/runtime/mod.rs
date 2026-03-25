//! 命令行与终端 UI 运行时，以及二者与 `api` 共用的终端侧能力（`latex_unicode`）、会话文件/导出（`chat_export`）。
//! `benchmark` 子模块提供批量无人值守测评能力（SWE-bench / GAIA / HumanEval 等）。
pub mod benchmark;
pub mod chat_export;
pub mod cli;
pub mod latex_unicode;
pub(crate) mod message_display;
pub(crate) mod plan_section;
pub(crate) mod terminal_cli_transcript;
pub(crate) mod terminal_labels;
pub mod tui;
pub mod workspace_session;
