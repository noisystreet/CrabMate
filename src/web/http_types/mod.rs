//! Web HTTP JSON 契约中仍由 `web` 侧持有的部分（仅 serde 类型，无 handler），供 `routes::*` 与
//! `chat_handlers` / `workspace` 等共享，避免 `routes` ↔ handler 模块环依赖。
//!
//! `skills` / `tasks` / `tool_jobs` / `workspace` 的 1 跳再导出壳已删除；这些 DTO 的真源分别是
//! `cm_web_host::http_types::skills`、`cm_tools::workspace::tasks_side`、
//! `cm_api_contract::{tool_jobs, workspace}`，请直接引用。

pub mod chat;
pub mod github;
pub mod memory;
pub mod tools;
pub(crate) mod validation;
