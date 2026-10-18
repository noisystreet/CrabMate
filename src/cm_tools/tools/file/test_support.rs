//! `file` 子模块测试共用的夹具（仅 `#[cfg(test)]`）。

use crate::cm_config::WebSearchProvider;
use crate::cm_tools::tools::ToolContext;
use std::path::Path;

pub(super) fn test_ctx(working_dir: &Path) -> ToolContext<'_> {
    ToolContext {
        cfg: None,
        codebase_semantic_host: None,
        command_max_output_len: 1 << 20,
        weather_timeout_secs: 5,
        allowed_commands: &[],
        working_dir,
        web_search_timeout_secs: 5,
        web_search_provider: WebSearchProvider::Worbrow,
        web_search_api_key: "",
        web_search_max_results: 3,
        web_search_proxy: "",
        http_fetch_allowed_prefixes: &[],
        http_fetch_timeout_secs: 5,
        http_fetch_max_response_bytes: 1024,
        command_timeout_secs: 5,
        read_file_turn_cache: None,
        workspace_changelist: None,
        test_result_cache_enabled: false,
        test_result_cache_max_entries: 0,
        long_term_memory_host: None,
        tool_jobs_host: None,
    }
}

pub(super) fn write(ws: &Path, rel: &str, content: &str) {
    let p = ws.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, content).unwrap();
}
