//! Web HTTP JSON 契约（仅 serde 类型，无 handler），供 `routes::*` 与 `chat_handlers` / `workspace` 等共享，避免 `routes` ↔ handler 模块环依赖。

pub mod chat;
pub mod tasks;
pub mod workspace;
