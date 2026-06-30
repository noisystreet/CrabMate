//! [Model Context Protocol](https://modelcontextprotocol.io/) 入口。
//!
//! - **`mcp` feature**：stdio 客户端/服务端、进程内工具合并（默认构建经根 crate `default` 启用）。
//! - **关闭时**（`--no-default-features` 且不加 `mcp`）：不链接 **`rmcp`**；`mcp list` / `mcp serve` 不可用，配置中的 MCP 工具名会被忽略。

pub mod resolve;
pub use resolve::resolve_mcp_config;

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
