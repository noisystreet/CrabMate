//! CrabMate 运行时工具层：运维 CLI / 展示共用的独立模块（退出码、Unicode 转换、消息展示等）。
//!
//! 各模块无 `crabmate` 根包依赖，可独立编译。

pub mod cli_exit;
pub mod latex_unicode;

pub mod chat_export;
pub mod message_display;
pub mod message_display_parts;
pub mod message_snapshot_display;
