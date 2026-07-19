//! MCP (Model Context Protocol) 相关共享类型。

use serde::{Deserialize, Serialize};

/// 单条 MCP 远端工具的摘要（名称 + 可选的描述），用于状态展示而非执行。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoteToolSummary {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
