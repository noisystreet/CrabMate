//! CrabMate 运行时工具层：CLI/TUI 共用的独立模块（退出码、Unicode 转换、进度条、补全等）。
//!
//! 各模块无 `crabmate` 根包依赖，可独立编译。

pub mod cli_exit;
pub mod cli_wait_spinner;
pub mod latex_unicode;
pub mod plan_section;

pub mod chat_export;
pub mod message_display;
pub mod message_display_parts;
pub mod message_snapshot_display;

// `repl_slash_complete` 与 `tables` 模块待后续步骤迁移
// #[cfg(feature = "repl")]
// pub mod repl_slash_complete;
// pub mod tables;
