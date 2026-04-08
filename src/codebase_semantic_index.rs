//! 工作区代码语义索引：SQLite 存文本块 + fastembed 向量，供 `codebase_semantic_search` 工具使用。
//! 与长期记忆分库；`workspace_root` 为规范路径字符串，用于多工作区隔离（见 `docs/CODEBASE_INDEX_PLAN.md`）。

use crate::config::AgentConfig;

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
}

fn default_semantic_rebuild_incremental() -> bool {
    true
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
            query_max_chunks: cfg.codebase_semantic_query_max_chunks,
            rebuild_max_files: cfg.codebase_semantic_rebuild_max_files,
            rebuild_incremental: cfg.codebase_semantic_rebuild_incremental,
        }
    }
}

use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use ignore::WalkBuilder;
use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::tools::canonical_workspace_root;

const TABLE: &str = "crabmate_codebase_chunks";
const TABLE_FILES: &str = "crabmate_codebase_files";
/// 供失效逻辑删除文件目录表（与 chunks 同步）。
pub(crate) const CODEBASE_SEMANTIC_FILES_TABLE: &str = TABLE_FILES;
const SCHEMA_VERSION: i64 = 3;

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
        CREATE INDEX IF NOT EXISTS idx_{TABLE}_ws_rel ON {TABLE}(workspace_root, rel_path);
        CREATE TABLE IF NOT EXISTS {TABLE_FILES} (
            workspace_root TEXT NOT NULL,
            rel_path TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime_ns INTEGER NOT NULL,
            content_sha256 TEXT NOT NULL,
            PRIMARY KEY (workspace_root, rel_path)
        );
        CREATE INDEX IF NOT EXISTS idx_{TABLE_FILES}_ws ON {TABLE_FILES}(workspace_root);
        "#
    ))?;

    let ver: Option<i64> = conn
        .query_row(
            "SELECT value FROM crabmate_codebase_index_meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|s| s.parse().ok());

    if ver.unwrap_or(0) < SCHEMA_VERSION {
        let _ = conn.execute(
            &format!(
                "CREATE INDEX IF NOT EXISTS idx_{TABLE}_ws_rel ON {TABLE}(workspace_root, rel_path)"
            ),
            [],
        );
        let _ = conn.execute_batch(&format!(
            r#"
            CREATE TABLE IF NOT EXISTS {TABLE_FILES} (
                workspace_root TEXT NOT NULL,
                rel_path TEXT NOT NULL,
                size INTEGER NOT NULL,
                mtime_ns INTEGER NOT NULL,
                content_sha256 TEXT NOT NULL,
                PRIMARY KEY (workspace_root, rel_path)
            );
            CREATE INDEX IF NOT EXISTS idx_{TABLE_FILES}_ws ON {TABLE_FILES}(workspace_root);
            "#
        ));
        conn.execute(
            "INSERT OR REPLACE INTO crabmate_codebase_index_meta (key, value) VALUES ('schema_version', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
    }

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
    let base = canonical_workspace_root(workspace_root).map_err(|e| e.user_message())?;
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

/// `rebuild_index` 的 `path`：用于 DELETE 的 POSIX 前缀（无首尾 `/`，`.` 或空视为整库）。
fn posix_subdir_prefix_for_delete(sub: &str) -> Option<String> {
    let s = sub.replace('\\', "/");
    let s = s.trim().trim_matches('/');
    if s.is_empty() || s == "." {
        None
    } else {
        Some(s.to_string())
    }
}

fn sqlite_like_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            o.push('\\');
        }
        o.push(ch);
    }
    o
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
            incremental,
        );
    }

    search_index(
        &ws_key,
        &index_path,
        query,
        top_k_req,
        query_max_chunks,
        max_output_chars.max(4096),
    )
}

fn file_fingerprint(path: &Path, max_file_bytes: usize) -> Option<(u64, i64, String, String)> {
    let meta = fs::metadata(path).ok()?;
    let size = meta.len();
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| {
            let n = d.as_nanos();
            if n > i64::MAX as u128 {
                i64::MAX
            } else {
                n as i64
            }
        })
        .unwrap_or(0);
    let f = fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(max_file_bytes as u64 + 1)
        .read_to_end(&mut buf)
        .ok()?;
    if buf.len() > max_file_bytes {
        return None;
    }
    let text = String::from_utf8(buf).ok()?;
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    let hex = format!("{:x}", h.finalize());
    Some((size, mtime_ns, text, hex))
}

static RUST_FN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").expect("regex")
});
static RUST_TYPE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub\s+)?(?:struct|enum|trait|type)\s+([A-Za-z_][A-Za-z0-9_]*)")
        .expect("regex")
});
static RUST_IMPL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^\s*(?:pub\s+)?impl(?:<[^>]+>)?\s+([A-Za-z_][A-Za-z0-9_:]*)").expect("regex")
});

fn rust_symbol_hints_for_chunk(chunk: &str) -> String {
    let mut names: Vec<String> = Vec::new();
    for cap in RUST_FN_RE.captures_iter(chunk) {
        if let Some(m) = cap.get(1) {
            names.push(m.as_str().to_string());
        }
    }
    for cap in RUST_TYPE_RE.captures_iter(chunk) {
        if let Some(m) = cap.get(1) {
            names.push(m.as_str().to_string());
        }
    }
    for cap in RUST_IMPL_RE.captures_iter(chunk) {
        if let Some(m) = cap.get(1) {
            let s = m.as_str();
            if !s.starts_with("for ") {
                names.push(s.to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    if names.is_empty() {
        String::new()
    } else {
        let mut s = names.join(", ");
        const MAX: usize = 400;
        if s.len() > MAX {
            s.truncate(MAX);
            s.push('…');
        }
        format!("symbols: {}", s)
    }
}

fn embed_doc_for_chunk(rel: &str, ext: &str, chunk: &str) -> String {
    let hints = if ext == "rs" {
        rust_symbol_hints_for_chunk(chunk)
    } else {
        String::new()
    };
    if hints.is_empty() {
        format!("file: {}\n{}", rel, chunk)
    } else {
        format!("file: {}\n{}\n{}", rel, hints, chunk)
    }
}

#[allow(clippy::too_many_arguments)]
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
    incremental: bool,
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
    let delete_scope = sub_path.and_then(posix_subdir_prefix_for_delete);
    let subtree = delete_scope.is_some();

    match delete_scope.as_deref() {
        None | Some("") | Some(".") => {
            if !incremental {
                if let Err(e) = tx.execute(
                    &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1"),
                    params![ws_key],
                ) {
                    return format!("清空旧向量块失败: {}", e);
                }
                if let Err(e) = tx.execute(
                    &format!("DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1"),
                    params![ws_key],
                ) {
                    return format!("清空文件目录失败: {}", e);
                }
            }
        }
        Some(prefix) => {
            let like_pat = sqlite_like_escape(&format!("{prefix}/%"));
            if let Err(e) = tx.execute(
                &format!(
                    "DELETE FROM {TABLE} WHERE workspace_root = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 ESCAPE '\\')"
                ),
                params![ws_key, prefix, like_pat],
            ) {
                return format!("清空子树旧向量块失败: {}", e);
            }
            if let Err(e) = tx.execute(
                &format!(
                    "DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 ESCAPE '\\')"
                ),
                params![ws_key, prefix, like_pat],
            ) {
                return format!("清空子树文件目录失败: {}", e);
            }
        }
    }

    let mut embedder = match ensure_embedder() {
        Ok(m) => m,
        Err(e) => return e,
    };

    let mut catalog: HashMap<String, (u64, i64, String)> = HashMap::new();
    if incremental && !subtree {
        let mut stmt = match tx.prepare_cached(&format!(
            "SELECT rel_path, size, mtime_ns, content_sha256 FROM {TABLE_FILES} WHERE workspace_root = ?1"
        )) {
            Ok(s) => s,
            Err(e) => return format!("读取文件目录失败: {}", e),
        };
        let rows = match stmt.query_map(params![ws_key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        }) {
            Ok(it) => it,
            Err(e) => return format!("遍历文件目录失败: {}", e),
        };
        for (rel, sz, mt, sha) in rows.flatten() {
            catalog.insert(rel, (sz, mt, sha));
        }
    }

    let walker = WalkBuilder::new(&search_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();

    let mut files_indexed = 0usize;
    let mut files_unchanged = 0usize;
    let mut chunks_total = 0usize;
    let mut skipped_files = 0usize;
    let mut embed_batches: Vec<(String, String, usize, usize, String, String)> = Vec::new();
    let mut seen_rels: HashSet<String> = HashSet::new();
    let mut file_rows: Vec<(String, u64, i64, String)> = Vec::new();

    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
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
        seen_rels.insert(rel.clone());

        let Some((size, mtime_ns, text, sha_hex)) = file_fingerprint(path, max_file_bytes) else {
            if incremental && !subtree {
                let _ = tx.execute(
                    &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"),
                    params![ws_key, rel.as_str()],
                );
                let _ = tx.execute(
                    &format!(
                        "DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1 AND rel_path = ?2"
                    ),
                    params![ws_key, rel.as_str()],
                );
            }
            skipped_files = skipped_files.saturating_add(1);
            continue;
        };
        if incremental && !subtree {
            if let Some((sz, mt, sh)) = catalog.get(&rel)
                && *sz == size
                && *mt == mtime_ns
                && *sh == sha_hex
            {
                files_unchanged += 1;
                continue;
            }
            if let Err(e) = tx.execute(
                &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"),
                params![ws_key, rel.as_str()],
            ) {
                return format!("删除旧块失败: {}", e);
            }
        }

        if files_indexed >= rebuild_max_files {
            skipped_files = skipped_files.saturating_add(1);
            continue;
        }

        let mut file_chunks = 0usize;
        for (sl, el, chunk) in chunk_text_lines(&text, chunk_max_chars) {
            if chunk.chars().count() < 8 {
                continue;
            }
            let h = hash_chunk(&rel, &chunk);
            embed_batches.push((rel.clone(), h, sl, el, chunk, ext.clone()));
            file_chunks += 1;
        }
        if file_chunks > 0 {
            files_indexed += 1;
            file_rows.push((rel, size, mtime_ns, sha_hex));
        } else {
            if incremental && !subtree {
                let _ = tx.execute(
                    &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"),
                    params![ws_key, rel.as_str()],
                );
                let _ = tx.execute(
                    &format!(
                        "DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1 AND rel_path = ?2"
                    ),
                    params![ws_key, rel.as_str()],
                );
            }
            skipped_files = skipped_files.saturating_add(1);
        }
    }

    if incremental && !subtree {
        let stale: Vec<String> = catalog
            .keys()
            .filter(|k| !seen_rels.contains(*k))
            .cloned()
            .collect();
        for rel in stale {
            if let Err(e) = tx.execute(
                &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"),
                params![ws_key, rel.as_str()],
            ) {
                return format!("删除已删除文件的块失败: {}", e);
            }
            if let Err(e) = tx.execute(
                &format!("DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1 AND rel_path = ?2"),
                params![ws_key, rel.as_str()],
            ) {
                return format!("删除文件目录行失败: {}", e);
            }
        }
    }

    const BATCH: usize = 32;
    let mut i = 0;
    while i < embed_batches.len() {
        let end = (i + BATCH).min(embed_batches.len());
        let docs: Vec<String> = embed_batches[i..end]
            .iter()
            .map(|(rel, _, _, _, body, ext)| embed_doc_for_chunk(rel, ext, body))
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
            let (rel, h, sl, el, body, _) = &embed_batches[i + j];
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

    for (rel, sz, mt, sha) in file_rows {
        if let Err(e) = tx.execute(
            &format!(
                "INSERT OR REPLACE INTO {TABLE_FILES} (workspace_root, rel_path, size, mtime_ns, content_sha256) VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![ws_key, rel, sz as i64, mt, sha],
        ) {
            return format!("写入文件目录失败: {}", e);
        }
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

    let scope_note = match sub_path.and_then(posix_subdir_prefix_for_delete) {
        None => "范围：整库（未指定 path 或 path 为 .）".to_string(),
        Some(p) => format!("范围：子树 `{}`（其余路径索引保留）", p),
    };
    let mode_note = if subtree {
        "模式：子树全量重嵌入（已清空该子树目录表与块）。".to_string()
    } else if incremental {
        format!(
            "模式：整库增量（mtime+size+SHA256 未变的文件跳过嵌入；未再出现的文件已删块与目录行）。未改文件数：{}",
            files_unchanged
        )
    } else {
        "模式：整库全量（已清空向量块与文件目录后重建）。".to_string()
    };
    format!(
        "代码语义索引已重建。\n索引文件：{}\n工作区键：{}\n{}\n{}\n已嵌入文件数（本趟；上限 {}）：{}\n文本块数（本趟写入）：{}\n跳过/超限/未产生块：{}\n提示：大仓可调高 codebase_semantic_rebuild_max_files 或缩小 path/extensions；整库默认增量见 codebase_semantic_rebuild_incremental；强制全量可传 incremental:false。",
        index_path.display(),
        ws_key,
        scope_note,
        mode_note,
        rebuild_max_files,
        files_indexed,
        chunks_total,
        skipped_files
    )
}

#[derive(Clone)]
struct ScoredChunk {
    score: f32,
    rel: String,
    sl: i64,
    el: i64,
    text: String,
}

impl Eq for ScoredChunk {}

impl PartialEq for ScoredChunk {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Ord for ScoredChunk {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| self.rel.cmp(&other.rel))
            .then_with(|| self.sl.cmp(&other.sl))
    }
}

impl PartialOrd for ScoredChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn search_index(
    ws_key: &str,
    index_path: &Path,
    query: &str,
    top_k: usize,
    query_max_chunks: usize,
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

    let mut heap: BinaryHeap<Reverse<ScoredChunk>> = BinaryHeap::new();
    let mut scanned = 0usize;
    let limit_active = query_max_chunks > 0;
    for row in rows {
        if limit_active && scanned >= query_max_chunks {
            break;
        }
        let (rel, sl, el, text, blob) = match row {
            Ok(x) => x,
            Err(_) => continue,
        };
        scanned = scanned.saturating_add(1);
        let score = bytes_to_f32_slice(&blob)
            .map(|ev| cosine_sim(&qv, &ev))
            .unwrap_or(0.0);
        let item = ScoredChunk {
            score,
            rel,
            sl,
            el,
            text,
        };
        if heap.len() < top_k {
            heap.push(Reverse(item));
        } else if let Some(Reverse(worst)) = heap.peek()
            && item.score > worst.score
        {
            heap.pop();
            heap.push(Reverse(item));
        }
    }

    if heap.is_empty() {
        return format!(
            "索引中无条目（workspace_root={}）。请先使用 rebuild_index=true 构建索引。",
            ws_key
        );
    }

    let mut scored: Vec<ScoredChunk> = heap.into_iter().map(|r| r.0).collect();
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));

    let limit_note = if limit_active {
        format!("，上限 {}", query_max_chunks)
    } else {
        String::new()
    };
    let approx_note = if limit_active && scanned >= query_max_chunks {
        "（已达扫描上限，结果为近似 Top-K）"
    } else {
        ""
    };

    let mut out = String::new();
    out.push_str(&format!(
        "语义检索（top_k={}，已扫描 {} 个向量块{}{}，余弦相似度越高越相关）：\n\n",
        top_k, scanned, limit_note, approx_note,
    ));
    let mut used = 0usize;
    for (rank, chunk) in scored.iter().enumerate() {
        let header = format!(
            "## {}. {} (行 {}–{})  score={:.4}\n",
            rank + 1,
            chunk.rel,
            chunk.sl,
            chunk.el,
            chunk.score
        );
        let fence = "```\n";
        let footer = "```\n\n";
        let budget = max_out_chars.saturating_sub(used);
        if budget < header.len() + fence.len() + footer.len() + 20 {
            out.push_str("\n… 输出已达长度上限，后续结果已省略 …\n");
            break;
        }
        let remain = budget - header.len() - fence.len() - footer.len();
        let body = chunk.text.as_str();
        let snippet = if body.len() <= remain {
            body
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
