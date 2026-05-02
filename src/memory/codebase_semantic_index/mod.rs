//! 工作区代码语义索引：SQLite 存文本块 + fastembed 向量 + **FTS5** 全文索引（`content=` 外挂块表），
//! 供 `codebase_semantic_search` 工具使用。查询默认 **hybrid**：BM25（全文）与余弦（向量）加权融合。
//! 与长期记忆分库；`workspace_root` 为规范路径字符串，用于多工作区隔离（见 `docs/代码库索引方案.md`）。

#![cfg_attr(
    not(feature = "fastembed"),
    allow(dead_code, unused_variables, clippy::needless_return)
)]

mod numeric;
mod params;
mod rebuild;
mod schema;

#[cfg(feature = "fastembed")]
pub(crate) use numeric::{bytes_to_f32_slice, cosine_sim, ensure_embedder};
pub(crate) use numeric::{fts5_match_expression, norm_scores_bm25};
pub use params::CodebaseSemanticToolParams;
pub(crate) use schema::{
    CODEBASE_SEMANTIC_FILES_TABLE, TABLE, TABLE_FTS, index_path_for_workspace,
    open_codebase_semantic_db,
};

use std::collections::HashSet;
use std::path::Path;

use crate::tools::canonical_workspace_root;
use numeric::default_code_extensions;
use rebuild::{RebuildIndexParams, rebuild_index};

/// `rebuild_index=true` 时扫描工作区并写入向量；否则仅查询（需已有索引）。
pub fn run_tool(
    args_json: &str,
    workspace_root: &Path,
    p: &CodebaseSemanticToolParams,
    max_output_chars: usize,
) -> String {
    if !p.enabled {
        return "错误：代码语义检索已在配置中关闭（codebase_semantic_search_enabled=false）"
            .to_string();
    }

    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(x) => x,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };

    let rebuild = v
        .get("rebuild_index")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let query = v.get("query").and_then(|q| q.as_str()).unwrap_or("").trim();
    if !rebuild && query.is_empty() {
        return "错误：query 不能为空（除非 rebuild_index=true）".to_string();
    }

    let sub_path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let ws_root: std::path::PathBuf = match canonical_workspace_root(workspace_root) {
        Ok(p) => p,
        Err(e) => return format!("错误：{}", e.user_message()),
    };
    let ws_key = ws_root.to_string_lossy().to_string();

    let index_path = match index_path_for_workspace(workspace_root, &p.index_sqlite_path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let file_glob = v
        .get("file_glob")
        .and_then(|g| g.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let file_glob_pat = file_glob
        .as_deref()
        .and_then(|g| glob::Pattern::new(g).ok());

    let mut top_k_req = v
        .get("top_k")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.top_k as u64) as usize;
    top_k_req = top_k_req.clamp(1, 64);

    let exts_cfg = v.get("extensions").and_then(|e| e.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|x| {
                x.as_str()
                    .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
            })
            .filter(|s| !s.is_empty())
            .collect::<HashSet<_>>()
    });

    let ext_set: HashSet<String> = exts_cfg.unwrap_or_else(|| {
        default_code_extensions()
            .into_iter()
            .map(|x| x.to_string())
            .collect()
    });

    let mut query_max_chunks = v
        .get("query_max_chunks")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.query_max_chunks as u64) as usize;
    if query_max_chunks > 0 {
        query_max_chunks = query_max_chunks.clamp(1, 2_000_000);
    }

    let incremental = v
        .get("incremental")
        .and_then(|x| x.as_bool())
        .unwrap_or(p.rebuild_incremental);

    if rebuild {
        return rebuild_index(RebuildIndexParams {
            ws_root: &ws_root,
            ws_key: &ws_key,
            index_path: &index_path,
            sub_path,
            max_file_bytes: p.max_file_bytes,
            chunk_max_chars: p.chunk_max_chars,
            rebuild_max_files: p.rebuild_max_files,
            ext_set: &ext_set,
            file_glob_pat: file_glob_pat.as_ref(),
            incremental,
        });
    }

    let retrieve_mode = v
        .get("retrieve_mode")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("hybrid");

    let mut fts_top_n = v
        .get("fts_top_n")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.fts_top_n as u64) as usize;
    fts_top_n = fts_top_n.clamp(1, 10_000);

    let mut hybrid_semantic_pool = v
        .get("hybrid_semantic_pool")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.hybrid_semantic_pool as u64) as usize;
    hybrid_semantic_pool = hybrid_semantic_pool.clamp(top_k_req, 10_000);

    let mut hybrid_alpha = v
        .get("hybrid_alpha")
        .and_then(|x| x.as_f64())
        .map(|a| a as f32)
        .unwrap_or(p.hybrid_alpha);
    if !hybrid_alpha.is_finite() {
        hybrid_alpha = p.hybrid_alpha;
    }
    hybrid_alpha = hybrid_alpha.clamp(0.0, 1.0);

    let mode = match search::RetrieveMode::parse(retrieve_mode) {
        Ok(m) => m,
        Err(e) => return e,
    };
    search::search_index(
        &ws_key,
        &index_path,
        query,
        search::SearchQueryParams {
            top_k: top_k_req,
            query_max_chunks,
            max_out_chars: max_output_chars.max(4096),
            mode,
            fts_top_n,
            hybrid_semantic_pool,
            hybrid_alpha,
        },
    )
}
mod search;

#[cfg(test)]
mod tests {
    use super::numeric::{
        chunk_text_lines, cosine_sim, fts5_match_expression, norm_scores_bm25,
        posix_subdir_prefix_for_delete, rust_symbol_hints_for_chunk, sqlite_like_escape,
    };

    #[test]
    fn chunk_lines_respects_max() {
        let s = "a\nb\nc\nd\n";
        let c = chunk_text_lines(s, 3);
        assert!(!c.is_empty());
    }

    #[test]
    fn cosine_orthogonal() {
        let a = vec![1.0f32, 0.0];
        let b = vec![0.0f32, 1.0];
        assert!(cosine_sim(&a, &b).abs() < 0.001);
    }

    #[test]
    fn posix_subdir_prefix_dot_means_full_rebuild() {
        assert_eq!(posix_subdir_prefix_for_delete("."), None);
        assert_eq!(posix_subdir_prefix_for_delete("  ./  "), None);
    }

    #[test]
    fn posix_subdir_prefix_trims_slashes() {
        assert_eq!(
            posix_subdir_prefix_for_delete("src/"),
            Some("src".to_string())
        );
    }

    #[test]
    fn sqlite_like_escape_escapes_wildcards() {
        assert_eq!(sqlite_like_escape("a%b_c\\"), "a\\%b\\_c\\\\");
    }

    #[test]
    fn fts5_match_expression_and_and_quotes() {
        assert_eq!(
            fts5_match_expression("foo bar").as_deref(),
            Some("\"foo\" AND \"bar\"")
        );
        assert_eq!(
            fts5_match_expression("say \"hi\"").as_deref(),
            Some("\"say\" AND \"\"\"hi\"\"\"")
        );
        assert!(fts5_match_expression("   ").is_none());
    }

    #[test]
    fn norm_scores_bm25_constant_ranks() {
        let m = norm_scores_bm25(&[(1, 0.5), (2, 0.5)]);
        assert!((m[&1] - 0.5).abs() < 0.01);
        assert!((m[&2] - 0.5).abs() < 0.01);
    }

    #[test]
    fn rust_symbol_hints_fn_struct_impl() {
        let c = r#"
impl MyType {
    pub fn do_work() {}
}
pub struct Other {}
"#;
        let h = rust_symbol_hints_for_chunk(c);
        assert!(h.contains("do_work"), "{}", h);
        assert!(h.contains("MyType"), "{}", h);
        assert!(h.contains("Other"), "{}", h);
    }
}
