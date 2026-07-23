//! 命令行运行时，以及与 `api` 共用的终端侧能力；`benchmark` 子模块提供批量无人值守测评能力。
//!
//! 部分独立工具模块已提取到 `crabmate-runtime` crate，在此重导出。

pub mod benchmark;
pub use crabmate_runtime::chat_export;
pub mod cli;
pub mod cli_doctor;
pub mod cli_exit;
pub(crate) mod cli_mcp;
#[cfg(any(feature = "repl", feature = "tui"))]
pub(crate) mod cli_repl_ui;
pub use crabmate_runtime::cli_wait_spinner;
pub mod cli_workflow;
pub(crate) mod config_reload;
pub use crabmate_runtime::latex_unicode;
pub(crate) use crabmate_runtime::message_display;
pub(crate) use crabmate_runtime::message_snapshot_display;
#[allow(unused_imports)]
pub use crabmate_runtime::plan_section;
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
