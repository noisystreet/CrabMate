//! `run_agent_turn` 前置步骤：读缓存句柄、工作区变更集与合并 MCP/动态工具后的工具表。

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::config::AgentConfig;
use crate::process_handles::ProcessHandles;
use crate::types::Tool;
use crate::workspace::changelist::WorkspaceChangelist;

pub(crate) struct ToolsForTurnPrepared {
    pub tools_for_turn: Vec<Tool>,
    pub mcp_session: Option<Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
}

pub(crate) fn resolve_read_file_turn_cache_for_turn(
    cfg: &AgentConfig,
    read_file_turn_cache: Option<Arc<crate::ReadFileTurnCache>>,
) -> Option<Arc<crate::ReadFileTurnCache>> {
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

pub(crate) fn workspace_changelist_for_turn(
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

pub(crate) async fn prepare_tools_for_turn(
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
    let mcp_session = match crate::mcp::try_open_session_and_tools(cfg.as_ref()).await {
        Some((sess, extra)) => {
            tools_for_turn = crate::mcp::merge_tool_lists(tools_for_turn, extra);
            Some(sess)
        }
        None => None,
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
        let mcp_ok = allow.contains("mcp");
        tools_for_turn.retain(|t| {
            let n = t.function.name.as_str();
            if n.starts_with("mcp__") {
                return mcp_ok;
            }
            allow.contains(n)
        });
    }
    ToolsForTurnPrepared {
        tools_for_turn,
        mcp_session,
    }
}
