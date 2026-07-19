//! [Model Context Protocol](https://modelcontextprotocol.io/) 入口。
//!
//! - **`mcp` feature**：stdio 客户端/服务端、进程内工具合并。
//! - **关闭时**（`--no-default-features` 且不加 `mcp`）：不链接 **`rmcp`**，调用返回明确错误。

pub mod resolve;
pub use resolve::resolve_mcp_config;

#[cfg(feature = "mcp")]
mod mcp_impl;
#[cfg(feature = "mcp")]
mod turn_handle;

#[cfg(feature = "mcp")]
pub use mcp_impl::*;
#[cfg(feature = "mcp")]
pub use turn_handle::{McpTurnHandle, McpTurnSessions};

#[cfg(not(feature = "mcp"))]
mod stub;

#[cfg(not(feature = "mcp"))]
pub use stub::*;
