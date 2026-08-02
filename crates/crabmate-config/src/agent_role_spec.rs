//! 单条 Agent 角色：合并后的 system 正文与可选工具白名单。

use std::collections::HashSet;
use std::sync::Arc;

use crabmate_types::SessionMode;

/// 配置加载完成后的角色规格（`id -> spec`）。
#[derive(Debug, Clone)]
pub struct AgentRoleSpec {
    pub system_prompt: String,
    /// `Some`：仅允许这些工具名；显式写 `"mcp"` 表示允许所有 `mcp__*`，亦可写完整 `mcp__…` 名精确放行。`None`：不限制（与未配置该项一致）。
    pub allowed_tools: Option<Arc<HashSet<String>>>,
    /// 该角色未显式选 mode、且会话未持久化 mode 时的默认工作模式。
    pub default_session_mode: Option<SessionMode>,
}
