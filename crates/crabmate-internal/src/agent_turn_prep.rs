//! `run_agent_turn` 前置步骤：读缓存句柄、工作区变更集与合并 MCP/动态工具后的工具表。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::process_handles::ProcessHandles;
use crate::read_file_turn_cache::ReadFileTurnCache;
use crate::workspace::changelist::WorkspaceChangelist;
use crabmate_config::AgentConfig;
use crabmate_types::Tool;

pub struct ToolsForTurnPrepared {
    pub tools_for_turn: Vec<Tool>,
    pub mcp_turn: Option<crate::mcp::McpTurnHandle>,
    /// 本轮尝试连接但失败的 MCP 服务器（供终端/SSE 提示）。
    pub mcp_skipped: Vec<crate::mcp::McpServerSkipInfo>,
}

pub fn resolve_read_file_turn_cache_for_turn(
    cfg: &AgentConfig,
    read_file_turn_cache: Option<Arc<ReadFileTurnCache>>,
) -> Option<Arc<ReadFileTurnCache>> {
    match read_file_turn_cache {
        Some(a) => Some(a),
        None if cfg.chat_queues_cache.read_file_turn_cache_max_entries > 0 => {
            Some(crate::read_file_turn_cache::new_turn_cache_handle(
                cfg.chat_queues_cache.read_file_turn_cache_max_entries,
            ))
        }
        None => None,
    }
}

pub fn workspace_changelist_for_turn(
    cfg: &AgentConfig,
    process_handles: &ProcessHandles,
    long_term_memory_scope_id: Option<&str>,
) -> Option<Arc<WorkspaceChangelist>> {
    if !cfg
        .session_workspace_changelist
        .session_workspace_changelist_enabled
    {
        return None;
    }
    let scope = long_term_memory_scope_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("__default__");
    Some(
        process_handles
            .workspace_changelist_registry
            .changelist_for_scope(scope),
    )
}

pub async fn prepare_tools_for_turn(
    cfg: &Arc<AgentConfig>,
    tools: &[Tool],
    effective_working_dir: &Path,
    turn_allowed_tool_names: Option<&HashSet<String>>,
) -> ToolsForTurnPrepared {
    let mut tools_for_turn: Vec<Tool> = tools.to_vec();
    tools_for_turn = crate::mcp::merge_tool_lists(
        tools_for_turn,
        crate::dynamic_tools::load_dynamic_tools(effective_working_dir),
    );
    let open = crate::mcp::try_open_session_and_tools(cfg.as_ref()).await;
    let mcp_skipped = open.skipped;
    let mcp_turn = match (open.handle, open.tools) {
        (Some(handle), extra) => {
            tools_for_turn = crate::mcp::merge_tool_lists(tools_for_turn, extra);
            Some(handle)
        }
        (None, _) => None,
    };
    if !cfg.codebase_semantic.codebase_semantic_search_enabled {
        tools_for_turn.retain(|t| t.function.name != "codebase_semantic_search");
    }
    if !cfg.long_term_memory.long_term_memory_enabled {
        tools_for_turn.retain(|t| {
            !matches!(
                t.function.name.as_str(),
                "long_term_remember" | "long_term_forget" | "long_term_memory_list"
            )
        });
    }
    if let Some(allow) = turn_allowed_tool_names {
        tools_for_turn.retain(|t| {
            crabmate_tools::tool_naming::tool_name_allowed_by_turn_allowlist(
                t.function.name.as_str(),
                Some(allow),
            )
        });
    }
    ToolsForTurnPrepared {
        tools_for_turn,
        mcp_turn,
        mcp_skipped,
    }
}
