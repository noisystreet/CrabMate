//! 命令行运行时，以及与 `api` 共用的终端侧能力（`latex_unicode`）、会话文件/导出（`chat_export`）；**`cli_wait_spinner`** 为可选首包等待动效（**`CM_CLI_WAIT_SPINNER`**，由 **`llm::api::stream_chat`** 触发）。
//! `benchmark` 子模块提供批量无人值守测评能力（SWE-bench / GAIA / HumanEval 等）。
pub mod benchmark;
pub mod chat_export;
pub mod cli;
pub mod cli_doctor;
pub mod cli_exit;
pub(crate) mod cli_mcp;
#[cfg(any(feature = "repl", feature = "tui"))]
pub(crate) mod cli_repl_ui;
pub(crate) mod cli_wait_spinner;
pub mod cli_workflow;
pub(crate) mod config_reload;
pub mod latex_unicode;
pub(crate) mod message_display;
pub(crate) mod message_display_parts;
pub(crate) mod message_snapshot_display;
pub(crate) mod plan_section;
#[cfg(feature = "repl")]
pub(crate) mod repl_reedline;
#[cfg(feature = "repl")]
pub(crate) mod repl_slash_complete;
#[cfg(any(feature = "repl", feature = "tui"))]
pub(crate) mod terminal_cli_transcript;
#[cfg(any(feature = "repl", feature = "tui"))]
pub(crate) mod terminal_labels;
pub mod tool_replay;
#[cfg(feature = "tui")]
pub mod tui;
#[cfg(feature = "tui")]
pub(crate) mod tui_terminal_bridge;
pub mod workspace_session;
