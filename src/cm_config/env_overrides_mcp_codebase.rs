//! `CM_MCP_*` 与 `CM_CODEBASE_SEMANTIC_*` 环境覆盖（从 `env_overrides.rs` 拆分以降低圈复杂度）。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_nonempty_opt, apply_parse};

pub(super) fn apply_env_overrides_part_14(b: &mut ConfigBuilder) {
    env_override_mcp_client_fields(b);
    env_override_codebase_semantic_fields(b);
}

fn env_override_mcp_client_fields(b: &mut ConfigBuilder) {
    apply_bool(&mut b.mcp_client.mcp_enabled, "CM_MCP_ENABLED");
    apply_nonempty_opt(&mut b.mcp_client.mcp_command, "CM_MCP_COMMAND");
    apply_parse(
        &mut b.mcp_client.mcp_tool_timeout_secs,
        "CM_MCP_TOOL_TIMEOUT_SECS",
    );
}

fn env_override_codebase_semantic_fields(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.codebase_semantic.codebase_semantic_search_enabled,
        "CM_CODEBASE_SEMANTIC_SEARCH_ENABLED",
    );
    apply_bool(
        &mut b
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change,
        "CM_CODEBASE_SEMANTIC_INVALIDATE_ON_WORKSPACE_CHANGE",
    );
    apply_nonempty_opt(
        &mut b.codebase_semantic.codebase_semantic_index_sqlite_path,
        "CM_CODEBASE_SEMANTIC_INDEX_SQLITE_PATH",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_max_file_bytes,
        "CM_CODEBASE_SEMANTIC_MAX_FILE_BYTES",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_chunk_max_chars,
        "CM_CODEBASE_SEMANTIC_CHUNK_MAX_CHARS",
    );
}
