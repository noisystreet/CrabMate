//! [`CodebaseSemanticToolParams`] 与 serde 默认值。

use crabmate_config::AgentConfig;

fn default_semantic_invalidate_on_change() -> bool {
    true
}

fn default_semantic_query_max_chunks() -> usize {
    50_000
}

/// 供 [`crate::tools::ToolContext`] 注入的语义检索参数（避免在工具层持有整份 [`AgentConfig`]）。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CodebaseSemanticToolParams {
    pub enabled: bool,
    /// 写工具成功后按路径删块或整表失效（与 `read_file` 缓存策略对齐）。
    #[serde(default = "default_semantic_invalidate_on_change")]
    pub invalidate_on_workspace_change: bool,
    pub index_sqlite_path: String,
    pub max_file_bytes: usize,
    pub chunk_max_chars: usize,
    pub top_k: usize,
    /// 单次 `query` 最多扫描多少个向量块（防超大索引拖慢 CPU）；`0` 表示不限制。
    #[serde(default = "default_semantic_query_max_chunks")]
    pub query_max_chunks: usize,
    pub rebuild_max_files: usize,
    /// `rebuild_index` 且未指定 `path`（整库）时：按文件 `mtime+size+SHA256` 跳过未改文件，仅重嵌入变更项（`incremental:false` 可强制全量）。
    #[serde(default = "default_semantic_rebuild_incremental")]
    pub rebuild_incremental: bool,
    /// `hybrid` 混合检索中向量余弦分的权重 α；最终 `α*cosine + (1-α)*fts_norm`。
    pub hybrid_alpha: f32,
    /// 混合 / `fts_only` 时 FTS 分支最多取多少行参与融合（按 BM25）。
    pub fts_top_n: usize,
    /// `hybrid` 时向量扫描阶段保留的候选块数（≥ top_k，用于与 FTS 结果并集重排）。
    pub hybrid_semantic_pool: usize,
}

fn default_semantic_rebuild_incremental() -> bool {
    true
}

impl CodebaseSemanticToolParams {
    pub fn from_agent_config(cfg: &AgentConfig) -> Self {
        Self {
            enabled: cfg.codebase_semantic.codebase_semantic_search_enabled,
            invalidate_on_workspace_change: cfg
                .codebase_semantic
                .codebase_semantic_invalidate_on_workspace_change,
            index_sqlite_path: cfg
                .codebase_semantic
                .codebase_semantic_index_sqlite_path
                .clone(),
            max_file_bytes: cfg.codebase_semantic.codebase_semantic_max_file_bytes,
            chunk_max_chars: cfg.codebase_semantic.codebase_semantic_chunk_max_chars,
            top_k: cfg.codebase_semantic.codebase_semantic_top_k,
            query_max_chunks: cfg.codebase_semantic.codebase_semantic_query_max_chunks,
            rebuild_max_files: cfg.codebase_semantic.codebase_semantic_rebuild_max_files,
            rebuild_incremental: cfg.codebase_semantic.codebase_semantic_rebuild_incremental,
            hybrid_alpha: cfg.codebase_semantic.codebase_semantic_hybrid_alpha,
            fts_top_n: cfg.codebase_semantic.codebase_semantic_fts_top_n,
            hybrid_semantic_pool: cfg.codebase_semantic.codebase_semantic_hybrid_semantic_pool,
        }
    }
}
