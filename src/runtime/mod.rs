//! 命令行运行时，以及与 `api` 共用的运行时侧能力；`benchmark` 子模块提供批量无人值守测评能力。
//!
//! 部分独立工具模块已提取到 `crabmate-runtime` crate，在此重导出。

pub mod benchmark;
pub use crabmate_runtime::chat_export;
pub mod cli;
pub mod cli_doctor;
pub mod cli_exit;
pub(crate) mod cli_mcp;
pub(crate) mod cli_web_bearer;
pub mod cli_workflow;
pub(crate) mod config_reload;
pub(crate) use crabmate_runtime::message_snapshot_display;
pub mod tool_replay;
pub mod workspace_session;
