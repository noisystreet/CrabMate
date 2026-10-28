//! 工作区代码语义索引：SQLite 存文本块 + fastembed 向量 + **FTS5** 全文索引（`content=` 外挂块表），
//! 供 `codebase_semantic_search` 工具使用。查询默认 **hybrid**：BM25（全文）与余弦（向量）加权融合。
//! 与长期记忆分库；`workspace_root` 为规范路径字符串，用于多工作区隔离（见 `docs/design/archive/代码库索引方案.md`）。

#![cfg_attr(
    not(feature = "fastembed"),
    allow(dead_code, unused_variables, clippy::needless_return)
)]

mod numeric;
mod params;
mod rebuild;
mod schema;

#[cfg(feature = "fastembed")]
pub use numeric::{bytes_to_f32_slice, cosine_sim, ensure_embedder};
pub use numeric::{fts5_match_expression, norm_scores_bm25};
pub use params::CodebaseSemanticToolParams;
pub use schema::{
    CODEBASE_SEMANTIC_FILES_TABLE, TABLE, TABLE_FTS, index_path_for_workspace,
    open_codebase_semantic_db,
};

use std::collections::HashSet;
use std::path::Path;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_types::path_utils::canonical_workspace_root;
use numeric::default_code_extensions;
use rebuild::{RebuildIndexParams, rebuild_index};

fn json_bool(v: &serde_json::Value, key: &str, default: bool) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(default)
}

fn parse_ext_set(v: &serde_json::Value) -> HashSet<String> {
    v.get("extensions")
        .and_then(|e| e.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| {
                    x.as_str()
                        .map(|s| s.trim().trim_start_matches('.').to_ascii_lowercase())
                })
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_else(|| {
            default_code_extensions()
                .into_iter()
                .map(ToString::to_string)
                .collect()
        })
}

fn parse_file_glob_pat(v: &serde_json::Value) -> Option<glob::Pattern> {
    v.get("file_glob")
        .and_then(|g| g.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|g| glob::Pattern::new(g).ok())
}

fn parse_query_max_chunks(v: &serde_json::Value, default: usize) -> usize {
    let mut n = v
        .get("query_max_chunks")
        .and_then(|x| x.as_u64())
        .unwrap_or(default as u64) as usize;
    if n > 0 {
        n = n.clamp(1, 2_000_000);
    }
    n
}

/// 工作区根或索引文件路径不可解析：换路径才行，属 `Workspace`。
fn workspace_resolve_failed(msg: String) -> ToolError {
    ToolError::workspace("codebase_semantic_workspace_resolve_failed", msg)
}

/// 索引侧失败（SQLite 打开 / 事务 / 扫描 / 嵌入）：`External`，正文保持原措辞。
///
/// 只在缺少 `错误：` 前缀时补上——旧实现把这些 `String` 原样回正文，
/// 没有前缀的那些会被 `looks_like_failure` 判成**成功**。
fn index_failed(msg: String) -> ToolError {
    let msg = if msg.starts_with("错误") {
        msg
    } else {
        format!("错误：{msg}")
    };
    ToolError::external_code("codebase_semantic_index_failed", msg)
}

#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn resolve_run_tool_paths(
    workspace_root: &Path,
    index_sqlite_path: &str,
) -> Result<(std::path::PathBuf, String, std::path::PathBuf), ToolError> {
    let ws_root = canonical_workspace_root(workspace_root)
        .map_err(|e| workspace_resolve_failed(format!("错误：{}", e.user_message())))?;
    let ws_key = ws_root.to_string_lossy().to_string();
    let index_path = index_path_for_workspace(workspace_root, index_sqlite_path).map_err(|e| {
        // 旧实现把这条 `String` 原样回正文（无「错误：」前缀）⇒ 被判成**成功**。
        let msg = if e.starts_with("错误") {
            e
        } else {
            format!("错误：{e}")
        };
        workspace_resolve_failed(msg)
    })?;
    Ok((ws_root, ws_key, index_path))
}

#[allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 runner 契约一致
fn parse_search_query_params(
    v: &serde_json::Value,
    p: &CodebaseSemanticToolParams,
    top_k: usize,
    query_max_chunks: usize,
    max_output_chars: usize,
) -> Result<search::SearchQueryParams, ToolError> {
    let retrieve_mode = v
        .get("retrieve_mode")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("hybrid");
    let fts_top_n = v
        .get("fts_top_n")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.fts_top_n as u64) as usize;
    let fts_top_n = fts_top_n.clamp(1, 10_000);
    let hybrid_semantic_pool = v
        .get("hybrid_semantic_pool")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.hybrid_semantic_pool as u64) as usize;
    let hybrid_semantic_pool = hybrid_semantic_pool.clamp(top_k, 10_000);
    let mut hybrid_alpha = v
        .get("hybrid_alpha")
        .and_then(|x| x.as_f64())
        .map(|a| a as f32)
        .unwrap_or(p.hybrid_alpha);
    if !hybrid_alpha.is_finite() {
        hybrid_alpha = p.hybrid_alpha;
    }
    hybrid_alpha = hybrid_alpha.clamp(0.0, 1.0);
    Ok(search::SearchQueryParams {
        top_k,
        query_max_chunks,
        max_out_chars: max_output_chars.max(4096),
        mode: search::RetrieveMode::parse(retrieve_mode).map_err(ToolError::invalid_args)?,
        fts_top_n,
        hybrid_semantic_pool,
        hybrid_alpha,
    })
}
/// `codebase_semantic_search` 执行面：`rebuild_index=true` 时重建索引，否则只查询。
///
/// 迁 `Typed`（2026-10-28）时清掉四处**静默成功**：参数 JSON 坏的 `参数 JSON 无效: …`、
/// `index_path_for_workspace` 原样回吐的路径错误、以及 `rebuild_index` / `search_index`
/// 内部以 `错误：…` 之外措辞返回的失败。前三者现在分别是 `invalid_args` /
/// `codebase_semantic_workspace_resolve_failed`；后者由两个入口的 `Result` 承载。
#[allow(clippy::result_large_err)]
pub fn run_tool_try(
    args_json: &str,
    workspace_root: &Path,
    p: &CodebaseSemanticToolParams,
    max_output_chars: usize,
) -> Result<String, ToolError> {
    if !p.enabled {
        return Err(ToolError::workspace(
            "codebase_semantic_unconfigured",
            "错误：代码语义检索已在配置中关闭（codebase_semantic_search_enabled=false）".to_string(),
        ));
    }

    let v: serde_json::Value = serde_json::from_str(args_json).map_err(|e| {
        ToolError::invalid_args(format!("错误：参数 JSON 无效: {e}"))
    })?;

    let rebuild = json_bool(&v, "rebuild_index", false);
    let query = v.get("query").and_then(|q| q.as_str()).unwrap_or("").trim();
    if !rebuild && query.is_empty() {
        return Err(ToolError::invalid_args(
            "错误：query 不能为空（除非 rebuild_index=true）".to_string(),
        ));
    }

    let (ws_root, ws_key, index_path) =
        resolve_run_tool_paths(workspace_root, &p.index_sqlite_path)?;

    let top_k_req = v
        .get("top_k")
        .and_then(|n| n.as_u64())
        .unwrap_or(p.top_k as u64) as usize;
    let top_k_req = top_k_req.clamp(1, 64);
    let query_max_chunks = parse_query_max_chunks(&v, p.query_max_chunks);

    if rebuild {
        let sub_path = v
            .get("path")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let file_glob_pat = parse_file_glob_pat(&v);
        let ext_set = parse_ext_set(&v);
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
            incremental: json_bool(&v, "incremental", p.rebuild_incremental),
        });
    }

    let q = parse_search_query_params(&v, p, top_k_req, query_max_chunks, max_output_chars)?;
    search::search_index(&ws_key, &index_path, query, q)
}
mod search;

#[cfg(test)]
mod tests {
    use super::numeric::{
        chunk_text_lines, cosine_sim, fts5_match_expression, norm_scores_bm25,
        posix_subdir_prefix_for_delete, rust_symbol_hints_for_chunk, sqlite_like_escape,
    };

    /// 参数校验用的最小配置（三条用例都在打开索引库之前返回，不碰磁盘）。
    fn test_params(enabled: bool) -> super::CodebaseSemanticToolParams {
        super::CodebaseSemanticToolParams {
            enabled,
            invalidate_on_workspace_change: true,
            index_sqlite_path: String::new(),
            max_file_bytes: 1,
            chunk_max_chars: 1,
            top_k: 4,
            query_max_chunks: 64,
            rebuild_max_files: 1,
            rebuild_incremental: true,
            hybrid_alpha: 0.5,
            fts_top_n: 8,
            hybrid_semantic_pool: 8,
        }
    }

    /// 坏 JSON 从前回 `参数 JSON 无效: …`（**无**「错误：」前缀）⇒ `looks_like_failure`
    /// 判 `ok: true`：模型传错参数却拿到「成功」。现在稳定为 `invalid_args`。
    #[test]
    fn bad_args_is_invalid_args_not_success() {
        let e = super::run_tool_try("{", std::path::Path::new("."), &test_params(true), 4096)
            .expect_err("坏 JSON 必须失败");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("参数 JSON 无效"), "{}", e.message);
    }

    /// `query` 缺失 / 全空白且不重建索引：是参数问题，不是「空结果」。
    #[test]
    fn blank_query_is_invalid_args() {
        for args in [r#"{}"#, r#"{"query":"   "}"#] {
            let e = super::run_tool_try(args, std::path::Path::new("."), &test_params(true), 4096)
                .expect_err(args);
            assert_eq!(e.code, "invalid_args", "{args}");
        }
    }

    /// 配置里关掉该工具：属**工作区前置条件**（换配置才修），不是参数错误。
    #[test]
    fn disabled_is_unconfigured_and_classified_as_workspace() {
        let e = super::run_tool_try(
            r#"{"query":"x"}"#,
            std::path::Path::new("."),
            &test_params(false),
            4096,
        )
        .expect_err("关闭时应失败");
        assert_eq!(e.code, "codebase_semantic_unconfigured");
        assert_eq!(
            e.category,
            crate::cm_tools::tool_result::ToolFailureCategory::Workspace
        );
    }

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
