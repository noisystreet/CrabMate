//! 单条 Agent 角色：合并后的 system 正文与可选工具白名单。

use std::collections::HashSet;
use std::sync::Arc;

/// 配置加载完成后的角色规格（`id -> spec`）。
#[derive(Debug, Clone)]
pub struct AgentRoleSpec {
    pub system_prompt: String,
    /// `Some`：仅允许这些工具名；显式写 `"mcp"` 表示允许所有 `mcp__*`。`None`：不限制（与未配置该项一致）。
    pub allowed_tools: Option<Arc<HashSet<String>>>,
}
