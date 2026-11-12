//! `mcp_client.*` 与 `codebase_semantic.*` 的 `CM_*` 覆盖。
//!
//! 两组都服务于「代码库检索链路」：`mcp_client` 是可选的外部工具通道，
//! `codebase_semantic` 是内建索引；字段分属两个配置段，组内顺序不影响结果。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_nonempty_opt, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    mcp_client_fields(b);
    codebase_semantic_index(b);
    codebase_semantic_retrieval(b);
}

fn mcp_client_fields(b: &mut ConfigBuilder) {
    apply_bool(&mut b.mcp_client.mcp_enabled, "CM_MCP_ENABLED");
    apply_parse(
        &mut b.mcp_client.mcp_tool_timeout_secs,
        "CM_MCP_TOOL_TIMEOUT_SECS",
    );
}

fn codebase_semantic_index(b: &mut ConfigBuilder) {
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

fn codebase_semantic_retrieval(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_top_k,
        "CM_CODEBASE_SEMANTIC_TOP_K",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_query_max_chunks,
        "CM_CODEBASE_SEMANTIC_QUERY_MAX_CHUNKS",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_rebuild_max_files,
        "CM_CODEBASE_SEMANTIC_REBUILD_MAX_FILES",
    );
    apply_bool(
        &mut b.codebase_semantic.codebase_semantic_rebuild_incremental,
        "CM_CODEBASE_SEMANTIC_REBUILD_INCREMENTAL",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_hybrid_alpha,
        "CM_CODEBASE_SEMANTIC_HYBRID_ALPHA",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_fts_top_n,
        "CM_CODEBASE_SEMANTIC_FTS_TOP_N",
    );
    apply_parse(
        &mut b.codebase_semantic.codebase_semantic_hybrid_semantic_pool,
        "CM_CODEBASE_SEMANTIC_HYBRID_SEMANTIC_POOL",
    );
}
