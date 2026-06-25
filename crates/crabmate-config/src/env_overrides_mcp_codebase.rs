//! `CM_MCP_*` 与 `CM_CODEBASE_SEMANTIC_*` 环境覆盖（从 `env_overrides.rs` 拆分以降低圈复杂度）。

use crate::builder::ConfigBuilder;
use crate::source::parse_bool_like;

pub(super) fn apply_env_overrides_part_14(b: &mut ConfigBuilder) {
    env_override_mcp_client_fields(b);
    env_override_codebase_semantic_fields(b);
}

fn env_override_mcp_client_fields(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_MCP_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.mcp_client.mcp_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_MCP_COMMAND") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.mcp_client.mcp_command = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_MCP_TOOL_TIMEOUT_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.mcp_client.mcp_tool_timeout_secs = Some(n);
    }
}

fn env_override_codebase_semantic_fields(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_SEARCH_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic.codebase_semantic_search_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_INVALIDATE_ON_WORKSPACE_CHANGE")
        && let Some(val) = parse_bool_like(&v)
    {
        b.codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change = Some(val);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.codebase_semantic.codebase_semantic_index_sqlite_path = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_MAX_FILE_BYTES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_max_file_bytes = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.codebase_semantic.codebase_semantic_chunk_max_chars = Some(n);
    }
}
