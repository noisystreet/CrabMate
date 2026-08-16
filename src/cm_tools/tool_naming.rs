//! 工具名前缀约定（MCP 代理、动态插件等），供策略层与 registry 共用。

use std::collections::HashSet;

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

/// 多角色 / 回合工具白名单判定（列表裁剪与执行层须共用本函数，避免分叉）。
///
/// - `allow == None`：不限制。
/// - 内置工具：须出现在集合中。
/// - `mcp__*`：集合含字面量 **`mcp`**（放行全部代理）**或**完整工具名时允许。
#[inline]
pub fn tool_name_allowed_by_turn_allowlist(name: &str, allow: Option<&HashSet<String>>) -> bool {
    let Some(set) = allow else {
        return true;
    };
    if is_mcp_proxy_tool(name) {
        return set.contains("mcp") || set.contains(name);
    }
    set.contains(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_allowlist_mcp_token_and_exact_name() {
        let mut by_token = HashSet::new();
        by_token.insert("mcp".to_string());
        assert!(tool_name_allowed_by_turn_allowlist(
            "mcp__fanalyzer__fanalyzer_watchlist_list",
            Some(&by_token)
        ));

        let mut by_exact = HashSet::new();
        by_exact.insert("mcp__fanalyzer__fanalyzer_watchlist_list".to_string());
        assert!(tool_name_allowed_by_turn_allowlist(
            "mcp__fanalyzer__fanalyzer_watchlist_list",
            Some(&by_exact)
        ));
        assert!(!tool_name_allowed_by_turn_allowlist(
            "mcp__fanalyzer__fanalyzer_analyze",
            Some(&by_exact)
        ));
        assert!(!tool_name_allowed_by_turn_allowlist(
            "read_file",
            Some(&by_exact)
        ));
    }
}
