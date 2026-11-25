//! `WorkflowConfig`（`cm_workflow` 的执行入参）从根 `AgentConfig` 的**唯一**派生点。
//!
//! 为什么要单独一层：`cm_workflow` 刻意不依赖 `crate::config`（它只认自己的 `WorkflowConfig`，
//! 见 `cm_workflow/config.rs` 的模块注释），于是「从 `AgentConfig` 搬字段」必然落在调用方。
//! 此前工具面（`agent/workflow_tool_dispatch.rs`）与运维 CLI（`runtime/cli_workflow.rs`）
//! 各抄了一份逐字相同的 34 行（#1383）——加字段时漏改一边**不会报错**，只会让同一个工作流在
//! 两个入口跑出不同的超时 / 白名单 / 检索参数。

use crate::cm_workflow::config::WorkflowConfig;
use crate::config::{AgentConfig, ExposeSecret};

/// 从运行期配置派生工作流执行配置。
///
/// `web_search_api_key` 在这里被复制成 `String`：这是 `WorkflowConfig` 的既有形状（搜索引擎
/// 适配层要它），**不要**在本函数或其调用点打印该值。
#[must_use]
pub fn workflow_config_from(cfg: &AgentConfig) -> WorkflowConfig {
    WorkflowConfig {
        command_timeout_secs: cfg.command_exec.command_timeout_secs,
        weather_timeout_secs: cfg.weather_tool.weather_timeout_secs,
        web_search_timeout_secs: cfg.web_search.web_search_timeout_secs,
        web_search_provider: cfg.web_search.web_search_provider.as_str().to_string(),
        web_search_api_key: cfg
            .web_search
            .web_search_api_key
            .expose_secret()
            .to_string(),
        web_search_max_results: cfg.web_search.web_search_max_results,
        web_search_proxy: cfg.web_search.web_search_proxy.clone(),
        http_fetch_timeout_secs: cfg.http_fetch.http_fetch_timeout_secs,
        http_fetch_max_response_bytes: cfg.http_fetch.http_fetch_max_response_bytes,
        http_fetch_allowed_prefixes: cfg.http_fetch.http_fetch_allowed_prefixes.clone(),
        allowed_commands: cfg.command_exec.allowed_commands.to_vec(),
        workspace_allowed_roots: cfg.workspace_roots.workspace_allowed_roots.clone(),
        command_max_output_len: cfg.command_exec.command_max_output_len,
        test_result_cache_enabled: cfg.chat_queues_cache.test_result_cache_enabled,
        test_result_cache_max_entries: cfg.chat_queues_cache.test_result_cache_max_entries,
        codebase_semantic_enabled: cfg.codebase_semantic.codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change: cfg
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change,
        codebase_semantic_index_sqlite_path: cfg
            .codebase_semantic
            .codebase_semantic_index_sqlite_path
            .clone(),
        codebase_semantic_max_file_bytes: cfg.codebase_semantic.codebase_semantic_max_file_bytes,
        codebase_semantic_chunk_max_chars: cfg.codebase_semantic.codebase_semantic_chunk_max_chars,
        codebase_semantic_top_k: cfg.codebase_semantic.codebase_semantic_top_k,
        codebase_semantic_query_max_chunks: cfg
            .codebase_semantic
            .codebase_semantic_query_max_chunks,
        codebase_semantic_rebuild_max_files: cfg
            .codebase_semantic
            .codebase_semantic_rebuild_max_files,
        codebase_semantic_rebuild_incremental: cfg
            .codebase_semantic
            .codebase_semantic_rebuild_incremental,
        codebase_semantic_hybrid_alpha: cfg.codebase_semantic.codebase_semantic_hybrid_alpha,
        codebase_semantic_fts_top_n: cfg.codebase_semantic.codebase_semantic_fts_top_n,
        codebase_semantic_hybrid_semantic_pool: cfg
            .codebase_semantic
            .codebase_semantic_hybrid_semantic_pool,
    }
}

#[cfg(test)]
mod tests {
    use super::workflow_config_from;

    /// 关键约束字段必须一一对上：这些字段决定「同一个工作流在两个入口跑出什么」，
    /// 漏搬一个就是静默不一致（`WorkflowConfig` 有 28 个字段，靠人眼对不齐）。
    #[test]
    fn maps_execution_critical_fields() {
        let mut cfg = crate::config::load_config(None).expect("embedded default config");
        cfg.command_exec.command_timeout_secs = 7;
        cfg.command_exec.command_max_output_len = 1234;
        cfg.command_exec.allowed_commands = vec!["cargo".to_string(), "git".to_string()].into();
        cfg.http_fetch.http_fetch_allowed_prefixes = vec!["http://127.0.0.1:8080".to_string()];
        cfg.http_fetch.http_fetch_max_response_bytes = 4321;
        cfg.web_search.web_search_timeout_secs = 11;
        cfg.web_search.web_search_max_results = 5;
        cfg.weather_tool.weather_timeout_secs = 13;
        cfg.chat_queues_cache.test_result_cache_enabled = true;
        cfg.codebase_semantic.codebase_semantic_top_k = 9;
        // S7：多根集合也必须搬过去——漏搬的表现是「节点内读同级仓仍被拒」，
        // 不会报错，所以必须落在这条「关键字段一一对上」的断言里。
        cfg.workspace_roots.workspace_allowed_roots = vec![std::path::PathBuf::from("/tmp/roots")];

        let wf = workflow_config_from(&cfg);
        assert_eq!(wf.command_timeout_secs, 7);
        assert_eq!(wf.command_max_output_len, 1234);
        assert_eq!(
            wf.allowed_commands,
            vec!["cargo".to_string(), "git".to_string()]
        );
        assert_eq!(
            wf.http_fetch_allowed_prefixes,
            vec!["http://127.0.0.1:8080"]
        );
        assert_eq!(wf.http_fetch_max_response_bytes, 4321);
        assert_eq!(wf.web_search_timeout_secs, 11);
        assert_eq!(wf.web_search_max_results, 5);
        assert_eq!(wf.weather_timeout_secs, 13);
        assert!(wf.test_result_cache_enabled);
        assert_eq!(wf.codebase_semantic_top_k, 9);
    }
}
