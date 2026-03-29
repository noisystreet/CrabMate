//! 工作区代码语义索引：SQLite 存文本块 + fastembed 向量，供 `codebase_semantic_search` 工具使用。
//! 与长期记忆分库；`workspace_root` 为规范路径字符串，用于多工作区隔离（见 `docs/CODEBASE_INDEX_PLAN.md`）。

use crate::config::AgentConfig;

fn default_semantic_invalidate_on_change() -> bool {
    true
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
    pub rebuild_max_files: usize,
}

impl CodebaseSemanticToolParams {
    pub fn from_agent_config(cfg: &AgentConfig) -> Self {
        Self {
            enabled: cfg.codebase_semantic_search_enabled,
            invalidate_on_workspace_change: cfg.codebase_semantic_invalidate_on_workspace_change,
            index_sqlite_path: cfg.codebase_semantic_index_sqlite_path.clone(),
            max_file_bytes: cfg.codebase_semantic_max_file_bytes,
            chunk_max_chars: cfg.codebase_semantic_chunk_max_chars,
            top_k: cfg.codebase_semantic_top_k,
            rebuild_max_files: cfg.codebase_semantic_rebuild_max_files,
        }
    }
}

use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use ignore::WalkBuilder;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::tools::canonical_workspace_root;

const TABLE: &str = "crabmate_codebase_chunks";
const SCHEMA_VERSION: i64 = 1;

fn migrate(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(&format!(
        r#"
        CREATE TABLE IF NOT EXISTS crabmate_codebase_index_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS {TABLE} (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            workspace_root TEXT NOT NULL,
            rel_path TEXT NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            chunk_text TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            embedding BLOB NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_{TABLE}_workspace ON {TABLE}(workspace_root);
        "#
    ))?;
    Ok(())
}

/// 打开或创建索引库并迁移 schema（不写日志全文）。
pub(crate) fn open_codebase_semantic_db(path: &Path) -> Result<Connection, String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("无法创建索引目录 {}: {}", parent.display(), e))?;
    }
    let conn = Connection::open(path)
        .map_err(|e| format!("无法打开代码语义索引库 {}: {}", path.display(), e))?;
    migrate(&conn).map_err(|e| format!("代码语义索引 schema 初始化失败: {}", e))?;
    Ok(conn)
}

pub(crate) fn index_path_for_workspace(
    workspace_root: &Path,
    configured: &str,
) -> Result<PathBuf, String> {
    let base = canonical_workspace_root(workspace_root)?;
    if configured.trim().is_empty() {
        return Ok(base.join(".crabmate/codebase_semantic.sqlite"));
    }
    let sub = configured.trim();
    if Path::new(sub).is_absolute() {
        return Err("codebase_semantic_index_sqlite_path 必须为相对工作区的相对路径".to_string());
    }
    let joined = base.join(sub);
    let canon = joined
        .canonicalize()
        .map_err(|e| format!("索引路径无法解析: {}", e))?;
    if !canon.starts_with(&base) {
        return Err("索引路径不能超出工作区根目录".to_string());
    }
    Ok(canon)
}

fn default_code_extensions() -> HashSet<&'static str> {
    [
        "rs", "toml", "md", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "java", "kt", "c",
        "cc", "cpp", "cxx", "h", "hpp", "json", "yaml", "yml", "sh", "bash", "css", "html", "vue",
        "svelte", "rb", "php", "swift", "scala", "clj", "ex", "exs", "erl", "hs", "ml", "mli",
        "fs", "fsi", "cs", "sql", "graphql", "proto",
    ]
    .into_iter()
    .collect()
}

fn chunk_text_lines(s: &str, max_chunk: usize) -> Vec<(usize, usize, String)> {
    let s = s.trim_end_matches('\n');
    if s.is_empty() || max_chunk == 0 {
        return Vec::new();
    }
    let lines: Vec<&str> = s.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < lines.len() {
        let start_line = i + 1;
        let mut chunk = String::new();
        let mut end_i = i;
        while end_i < lines.len() {
            let line = lines[end_i];
            let add_len = if chunk.is_empty() {
                line.len()
            } else {
                1 + line.len()
            };
            if !chunk.is_empty() && chunk.len() + add_len > max_chunk {
                break;
            }
            if !chunk.is_empty() {
                chunk.push('\n');
            }
            chunk.push_str(line);
            end_i += 1;
            if chunk.len() >= max_chunk {
                break;
            }
        }
        if end_i == i {
            // single line longer than max_chunk: hard split by chars
            let line = lines[i];
            let mut start = 0usize;
            while start < line.len() {
                let end = (start + max_chunk).min(line.len());
                let part = &line[start..end];
                let sl = i + 1;
                let el = i + 1;
                out.push((sl, el, part.to_string()));
                start = end;
            }
            i += 1;
            continue;
        }
        out.push((start_line, end_i, chunk));
        i = end_i;
    }
    out
}

fn hash_chunk(rel_path: &str, body: &str) -> String {
    let mut h = Sha256::new();
    h.update(rel_path.as_bytes());
    h.update(b"\0");
    h.update(body.as_bytes());
    format!("{:x}", h.finalize())
}

fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let d = na.sqrt() * nb.sqrt();
    if d <= f32::EPSILON { 0.0 } else { dot / d }
}

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn bytes_to_f32_slice(blob: &[u8]) -> Option<Vec<f32>> {
    if !blob.len().is_multiple_of(4) {
        return None;
    }
    let n = blob.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let chunk = blob.get(i * 4..i * 4 + 4)?;
        let arr: [u8; 4] = chunk.try_into().ok()?;
        out.push(f32::from_le_bytes(arr));
    }
    Some(out)
}

fn ensure_embedder() -> Result<TextEmbedding, String> {
    TextEmbedding::try_new(TextInitOptions::new(EmbeddingModel::AllMiniLML6V2))
        .map_err(|e| format!("fastembed 初始化失败: {}", e))
}

fn rel_path_for_workspace(workspace_root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(workspace_root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        Some(".".to_string())
    } else {
        Some(s)
    }
}

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
        Err(e) => return e,
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

    if rebuild {
        return rebuild_index(
            &ws_root,
            &ws_key,
            &index_path,
            sub_path,
            p.max_file_bytes,
            p.chunk_max_chars,
            p.rebuild_max_files,
            &ext_set,
            file_glob_pat.as_ref(),
        );
    }

    search_index(
        &ws_key,
        &index_path,
        query,
        top_k_req,
        max_output_chars.max(4096),
    )
}

#[allow(clippy::too_many_arguments)] // 重建扫描参数较多；与 `run_tool` 分层清晰
fn rebuild_index(
    ws_root: &Path,
    ws_key: &str,
    index_path: &Path,
    sub_path: Option<&str>,
    max_file_bytes: usize,
    chunk_max_chars: usize,
    rebuild_max_files: usize,
    ext_set: &HashSet<String>,
    file_glob_pat: Option<&glob::Pattern>,
) -> String {
    let mut conn = match open_codebase_semantic_db(index_path) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let search_root = match sub_path {
        None | Some(".") => ws_root.to_path_buf(),
        Some(s) => {
            if Path::new(s).is_absolute() {
                return "path 必须为相对于工作区的相对路径".to_string();
            }
            let joined = ws_root.join(s);
            let canon = match joined.canonicalize() {
                Ok(p) => p,
                Err(e) => return format!("path 无法解析: {}", e),
            };
            if !canon.starts_with(ws_root) {
                return "path 不能超出工作区根目录".to_string();
            }
            canon
        }
    };

    let tx = match conn.transaction() {
        Ok(t) => t,
        Err(e) => return format!("索引事务开始失败: {}", e),
    };
    if let Err(e) = tx.execute(
        &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1"),
        params![ws_key],
    ) {
        return format!("清空旧索引失败: {}", e);
    }

    let mut embedder = match ensure_embedder() {
        Ok(m) => m,
        Err(e) => return e,
    };

    let walker = WalkBuilder::new(&search_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();

    let mut files_indexed = 0usize;
    let mut chunks_total = 0usize;
    let mut skipped_files = 0usize;
    let mut embed_batches: Vec<(String, String, usize, usize, String)> = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if files_indexed >= rebuild_max_files {
            skipped_files = skipped_files.saturating_add(1);
            continue;
        }
        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }
        let path = entry.path();
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(pat) = file_glob_pat
            && !pat.matches(&name)
        {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext.is_empty() || !ext_set.contains(&ext) {
            continue;
        }

        let rel = match rel_path_for_workspace(ws_root, path) {
            Some(r) => r,
            None => continue,
        };

        let f = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let mut buf = Vec::new();
        if f.take(max_file_bytes as u64 + 1)
            .read_to_end(&mut buf)
            .is_err()
        {
            continue;
        }
        if buf.len() > max_file_bytes {
            skipped_files = skipped_files.saturating_add(1);
            continue;
        }
        let text = match String::from_utf8(buf) {
            Ok(s) => s,
            Err(_) => {
                skipped_files = skipped_files.saturating_add(1);
                continue;
            }
        };

        for (sl, el, chunk) in chunk_text_lines(&text, chunk_max_chars) {
            if chunk.chars().count() < 8 {
                continue;
            }
            let h = hash_chunk(&rel, &chunk);
            embed_batches.push((rel.clone(), h, sl, el, chunk));
        }
        files_indexed += 1;
    }

    const BATCH: usize = 32;
    let mut i = 0;
    while i < embed_batches.len() {
        let end = (i + BATCH).min(embed_batches.len());
        let docs: Vec<String> = embed_batches[i..end]
            .iter()
            .map(|(rel, _, _, _, body)| format!("file: {}\n{}", rel, body))
            .collect();
        let docs_ref: Vec<&str> = docs.iter().map(|s| s.as_str()).collect();
        let embeddings = match embedder.embed(docs_ref, None) {
            Ok(e) => e,
            Err(e) => return format!("嵌入批处理失败: {}", e),
        };
        if embeddings.len() != end - i {
            return "嵌入批处理返回维度不一致".to_string();
        }
        for (j, emb) in embeddings.into_iter().enumerate() {
            let blob = f32_slice_to_bytes(&emb);
            let (rel, h, sl, el, body) = &embed_batches[i + j];
            if let Err(e) = tx.execute(
                &format!(
                    "INSERT INTO {TABLE} (workspace_root, rel_path, start_line, end_line, chunk_text, content_hash, embedding) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                ),
                params![ws_key, rel, sl, el, body, h, blob],
            ) {
                return format!("写入索引失败: {}", e);
            }
            chunks_total += 1;
        }
        i = end;
    }

    if let Err(e) = tx.execute(
        "INSERT OR REPLACE INTO crabmate_codebase_index_meta (key, value) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    ) {
        return format!("写入元数据失败: {}", e);
    }

    if let Err(e) = tx.commit() {
        return format!("索引提交失败: {}", e);
    }

    format!(
        "代码语义索引已重建。\n索引文件：{}\n工作区键：{}\n已索引文件数（上限 {}）：{}\n文本块数：{}\n跳过/超限文件数：{}\n提示：之后用 query 检索；大仓可适当提高 codebase_semantic_rebuild_max_files 或缩小 path/extensions。",
        index_path.display(),
        ws_key,
        rebuild_max_files,
        files_indexed,
        chunks_total,
        skipped_files
    )
}

fn search_index(
    ws_key: &str,
    index_path: &Path,
    query: &str,
    top_k: usize,
    max_out_chars: usize,
) -> String {
    let conn = match open_codebase_semantic_db(index_path) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut embedder = match ensure_embedder() {
        Ok(m) => m,
        Err(e) => return e,
    };

    let q_emb = match embedder.embed(vec![format!("query: {}", query)], None) {
        Ok(mut v) => v.pop(),
        Err(e) => return format!("查询嵌入失败: {}", e),
    };
    let Some(qv) = q_emb else {
        return "查询嵌入失败: 空结果".to_string();
    };

    let mut stmt = match conn.prepare_cached(&format!(
        "SELECT rel_path, start_line, end_line, chunk_text, embedding FROM {TABLE} WHERE workspace_root = ?1"
    )) {
        Ok(s) => s,
        Err(e) => return format!("读取索引失败: {}", e),
    };

    let rows = match stmt.query_map(params![ws_key], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Vec<u8>>(4)?,
        ))
    }) {
        Ok(it) => it,
        Err(e) => return format!("遍历索引失败: {}", e),
    };

    let mut scored: Vec<(f32, String, i64, i64, String)> = Vec::new();
    for row in rows {
        let (rel, sl, el, text, blob) = match row {
            Ok(x) => x,
            Err(_) => continue,
        };
        let score = bytes_to_f32_slice(&blob)
            .map(|ev| cosine_sim(&qv, &ev))
            .unwrap_or(0.0);
        scored.push((score, rel, sl, el, text));
    }

    if scored.is_empty() {
        return format!(
            "索引中无条目（workspace_root={}）。请先使用 rebuild_index=true 构建索引。",
            ws_key
        );
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);

    let mut out = String::new();
    out.push_str(&format!(
        "语义检索（top_k={}，余弦相似度越高越相关）：\n\n",
        top_k
    ));
    let mut used = 0usize;
    for (rank, (score, rel, sl, el, body)) in scored.iter().enumerate() {
        let header = format!(
            "## {}. {} (行 {}–{})  score={:.4}\n",
            rank + 1,
            rel,
            sl,
            el,
            score
        );
        let fence = "```\n";
        let footer = "```\n\n";
        let budget = max_out_chars.saturating_sub(used);
        if budget < header.len() + fence.len() + footer.len() + 20 {
            out.push_str("\n… 输出已达长度上限，后续结果已省略 …\n");
            break;
        }
        let remain = budget - header.len() - fence.len() - footer.len();
        let snippet = if body.len() <= remain {
            body.as_str()
        } else {
            let take = remain.saturating_sub(20);
            if take > 0 {
                &body[..body
                    .char_indices()
                    .nth(take)
                    .map(|(i, _)| i)
                    .unwrap_or(body.len())]
            } else {
                ""
            }
        };
        out.push_str(&header);
        out.push_str(fence);
        out.push_str(snippet);
        if snippet.len() < body.len() {
            out.push_str("\n…(截断)…");
        }
        out.push_str(footer);
        used = out.len();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
