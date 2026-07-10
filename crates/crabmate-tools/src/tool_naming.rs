//! 工具名前缀约定（MCP 代理、动态插件等），供策略层与 registry 共用。

/// OpenAI 兼容 MCP 代理工具名前缀（`mcp__{slug}__{remote_name}`）。
pub const MCP_PROXY_PREFIX: &str = "mcp__";

/// 工作区 `plugins/*.json` 动态工具名前缀。
pub const DYNAMIC_TOOL_PREFIX: &str = "dyn__";

/// MCP 代理工具（`mcp__*`）；语义未知，默认禁止与内建只读工具并行同批。
#[inline]
pub fn is_mcp_proxy_tool(name: &str) -> bool {
    name.starts_with(MCP_PROXY_PREFIX)
}

/// 运行时动态工具（`dyn__*`）；语义不可静态证明，默认按写副作用处理。
#[inline]
pub fn is_dynamic_tool_name(name: &str) -> bool {
    name.starts_with(DYNAMIC_TOOL_PREFIX)
}
