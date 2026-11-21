/// 工作流执行所需的配置（从根 crate 的 AgentConfig 构造后传入）。
#[derive(Debug, Clone)]
pub struct WorkflowConfig {
    pub command_timeout_secs: u64,
    pub weather_timeout_secs: u64,
    pub web_search_timeout_secs: u64,
    pub web_search_provider: String,
    pub web_search_api_key: String,
    pub web_search_max_results: u32,
    /// worbrow 浏览器/静态抓取代理（空 = 直连/系统代理）。
    pub web_search_proxy: String,
    pub http_fetch_timeout_secs: u64,
    pub http_fetch_max_response_bytes: usize,
    pub http_fetch_allowed_prefixes: Vec<String>,
    pub allowed_commands: Vec<String>,
    /// S7：多根放行的根集合，供工作流节点内的文件工具决定解析基。
    ///
    /// 空表 = fail-closed 退化为 `SessionRoot`（见 `cm_types::path_scope_for`）；
    /// 工作流节点若漏搬该字段，表现是「节点内读同级仓仍被拒」而非静默放宽。
    pub workspace_allowed_roots: Vec<std::path::PathBuf>,
    pub command_max_output_len: usize,
    pub test_result_cache_enabled: bool,
    pub test_result_cache_max_entries: usize,
    pub codebase_semantic_enabled: bool,
    pub codebase_semantic_invalidate_on_workspace_change: bool,
    pub codebase_semantic_index_sqlite_path: String,
    pub codebase_semantic_max_file_bytes: usize,
    pub codebase_semantic_chunk_max_chars: usize,
    pub codebase_semantic_top_k: usize,
    pub codebase_semantic_query_max_chunks: usize,
    pub codebase_semantic_rebuild_max_files: usize,
    pub codebase_semantic_rebuild_incremental: bool,
    pub codebase_semantic_hybrid_alpha: f32,
    pub codebase_semantic_fts_top_n: usize,
    pub codebase_semantic_hybrid_semantic_pool: usize,
}
