//! [Model Context Protocol](https://modelcontextprotocol.io/) 入口。
//!
//! - **`mcp` feature**：stdio 客户端/服务端、进程内工具合并。
//! - **关闭时**（`--no-default-features` 且不加 `mcp`）：不链接 **`rmcp`**，调用返回明确错误。
//!
//! **S4-P1b（#1300）**：本模块即唯一本体。原 `cm_internal::mcp` 的 user-data 解析层
//! （`resolve_mcp_config`）已并入 [`resolve_config`]，那层按 feature 分两支的 27 个
//! re-export 随之删除；模块名同时去掉 `cm_` 前缀（`cm_mcp` → `mcp`），于是
//! `crate::mcp` / `crate::cm_mcp` / `crate::cm_internal::mcp` 三条旧路径收敛为一条。

use crate::cm_config::AgentConfig;

pub mod resolve;
pub mod schema_sanitize;
pub use schema_sanitize::sanitize_mcp_json_schema;
/// 原经 `cm_internal::mcp` 转出；并入本模块后仍从根暴露（`runtime/cli_mcp.rs` 用该路径）。
pub use resolve::classify_mcp_connect_error;

mod resolve_config;
pub use resolve_config::resolve_mcp_config;

#[cfg(feature = "mcp")]
mod mcp_impl;
#[cfg(feature = "mcp")]
mod turn_handle;

#[cfg(feature = "mcp")]
pub use mcp_impl::*;
#[cfg(feature = "mcp")]
pub use turn_handle::McpTurnHandle;

#[cfg(not(feature = "mcp"))]
mod stub;

#[cfg(not(feature = "mcp"))]
pub use stub::*;

/// 按当前 `AgentConfig` **与 user-data `mcp_servers.json`** 解析并打开 MCP 回合句柄。
///
/// 这是 agent 回合唯一应走的入口：先经 [`resolve_mcp_config`] 读取 user-data，
/// 再交给 feature 分支提供的 `try_open_turn_handle`。
pub async fn try_open_session_and_tools(cfg: &AgentConfig) -> McpTurnOpenResult {
    let resolved = resolve_mcp_config(cfg);
    try_open_turn_handle(&resolved).await
}
