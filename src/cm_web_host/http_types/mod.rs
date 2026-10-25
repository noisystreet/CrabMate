//! Web HTTP JSON 契约（仅 serde；无 axum handler）。
//!
//! 对话 / 工作区 / 工具任务等 DTO 的**真源**在 [`crate::cm_api_contract`]；本模块只保留
//! 由 HTTP 壳自有、尚未迁出的 `github` / `skills` / `limits` / `tools`。

pub mod github;
pub mod limits;
pub mod skills;
pub mod tools;
