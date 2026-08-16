//! 索引重建扫描与嵌入写入（需 `fastembed`）。

use std::collections::HashSet;
use std::path::Path;

#[cfg(feature = "fastembed")]
use std::collections::HashMap;

#[cfg(feature = "fastembed")]
use std::path::PathBuf;

#[cfg(feature = "fastembed")]
use std::fs;
#[cfg(feature = "fastembed")]
use std::io::Read;

#[cfg(feature = "fastembed")]
use fastembed::TextEmbedding;
#[cfg(feature = "fastembed")]
use ignore::WalkBuilder;
#[cfg(feature = "fastembed")]
use rusqlite::params;
#[cfg(feature = "fastembed")]
use sha2::{Digest, Sha256};

#[cfg(feature = "fastembed")]
use super::numeric::{
    chunk_text_lines, ensure_embedder, f32_slice_to_bytes, hash_chunk,
    posix_subdir_prefix_for_delete, rel_path_for_workspace, rust_symbol_hints_for_chunk,
    sqlite_like_escape,
};
#[cfg(feature = "fastembed")]
use super::schema::{SCHEMA_VERSION, TABLE, TABLE_FILES, open_codebase_semantic_db};
#[cfg(feature = "fastembed")]
use glob;

#[cfg(feature = "fastembed")]
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
    let hex: String = h.finalize().iter().map(|b| format!("{b:02x}")).collect();
    Some((size, mtime_ns, text, hex))
}

#[cfg(feature = "fastembed")]
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

#[cfg(feature = "fastembed")]
type EmbedBatchRow = (String, String, usize, usize, String, String);

#[cfg(feature = "fastembed")]
struct RebuildScanOutcome {
    files_indexed: usize,
    files_unchanged: usize,
    skipped_files: usize,
    embed_batches: Vec<EmbedBatchRow>,
    seen_rels: HashSet<String>,
    file_rows: Vec<(String, u64, i64, String)>,
}

#[cfg(feature = "fastembed")]
fn resolve_rebuild_search_root(ws_root: &Path, sub_path: Option<&str>) -> Result<PathBuf, String> {
    match sub_path {
        None | Some(".") => Ok(ws_root.to_path_buf()),
        Some(s) => {
            if Path::new(s).is_absolute() {
                return Err("path 必须为相对于工作区的相对路径".to_string());
            }
            let joined = ws_root.join(s);
            let canon = joined
                .canonicalize()
                .map_err(|e| format!("path 无法解析: {}", e))?;
            if !canon.starts_with(ws_root) {
                return Err("path 不能超出工作区根目录".to_string());
            }
            Ok(canon)
        }
    }
}

#[cfg(feature = "fastembed")]
fn clear_rebuild_scope_rows(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    sub_path: Option<&str>,
    incremental: bool,
) -> Result<bool, String> {
    let delete_scope = sub_path.and_then(posix_subdir_prefix_for_delete);
    let subtree = delete_scope.is_some();
    match delete_scope.as_deref() {
        None | Some("") | Some(".") => {
            if !incremental {
                tx.execute(
                    &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1"),
                    params![ws_key],
                )
                .map_err(|e| format!("清空旧向量块失败: {}", e))?;
                tx.execute(
                    &format!("DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1"),
                    params![ws_key],
                )
                .map_err(|e| format!("清空文件目录失败: {}", e))?;
            }
        }
        Some(prefix) => {
            let like_pat = sqlite_like_escape(&format!("{prefix}/%"));
            tx.execute(
                &format!(
                    "DELETE FROM {TABLE} WHERE workspace_root = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 ESCAPE '\\')"
                ),
                params![ws_key, prefix, like_pat],
            )
            .map_err(|e| format!("清空子树旧向量块失败: {}", e))?;
            tx.execute(
                &format!(
                    "DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1 AND (rel_path = ?2 OR rel_path LIKE ?3 ESCAPE '\\')"
                ),
                params![ws_key, prefix, like_pat],
            )
            .map_err(|e| format!("清空子树文件目录失败: {}", e))?;
        }
    }
    Ok(subtree)
}

#[cfg(feature = "fastembed")]
fn load_incremental_catalog(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    incremental: bool,
    subtree: bool,
) -> Result<HashMap<String, (u64, i64, String)>, String> {
    let mut catalog: HashMap<String, (u64, i64, String)> = HashMap::new();
    if !incremental || subtree {
        return Ok(catalog);
    }
    let mut stmt = tx
        .prepare_cached(&format!(
            "SELECT rel_path, size, mtime_ns, content_sha256 FROM {TABLE_FILES} WHERE workspace_root = ?1"
        ))
        .map_err(|e| format!("读取文件目录失败: {}", e))?;
    let rows = stmt
        .query_map(params![ws_key], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)? as u64,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })
        .map_err(|e| format!("遍历文件目录失败: {}", e))?;
    for (rel, sz, mt, sha) in rows.flatten() {
        catalog.insert(rel, (sz, mt, sha));
    }
    Ok(catalog)
}

#[cfg(feature = "fastembed")]
fn delete_rows_for_rel(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    rel: &str,
    chunk_err: &str,
    file_err: &str,
) -> Result<(), String> {
    tx.execute(
        &format!("DELETE FROM {TABLE} WHERE workspace_root = ?1 AND rel_path = ?2"),
        params![ws_key, rel],
    )
    .map_err(|e| format!("{chunk_err}: {}", e))?;
    tx.execute(
        &format!("DELETE FROM {TABLE_FILES} WHERE workspace_root = ?1 AND rel_path = ?2"),
        params![ws_key, rel],
    )
    .map_err(|e| format!("{file_err}: {}", e))?;
    Ok(())
}

#[cfg(feature = "fastembed")]
struct ScanRebuildFilesParams<'a> {
    ws_root: &'a Path,
    ws_key: &'a str,
    tx: &'a rusqlite::Transaction<'a>,
    search_root: &'a Path,
    max_file_bytes: usize,
    chunk_max_chars: usize,
    rebuild_max_files: usize,
    ext_set: &'a HashSet<String>,
    file_glob_pat: Option<&'a glob::Pattern>,
    incremental: bool,
    subtree: bool,
    catalog: &'a HashMap<String, (u64, i64, String)>,
}

#[cfg(feature = "fastembed")]
struct ScanRebuildFileLoopState<'a> {
    files_indexed: &'a mut usize,
    files_unchanged: &'a mut usize,
    skipped_files: &'a mut usize,
    embed_batches: &'a mut Vec<EmbedBatchRow>,
    file_rows: &'a mut Vec<(String, u64, i64, String)>,
}

#[cfg(feature = "fastembed")]
fn scan_rebuild_handle_missing_fingerprint(
    p: &ScanRebuildFilesParams<'_>,
    rel: &str,
    st: &mut ScanRebuildFileLoopState<'_>,
) {
    if p.incremental && !p.subtree {
        let _ = delete_rows_for_rel(
            p.tx,
            p.ws_key,
            rel,
            "删除不可索引文件的旧块失败",
            "删除不可索引文件目录行失败",
        );
    }
    *st.skipped_files = st.skipped_files.saturating_add(1);
}

#[cfg(feature = "fastembed")]
fn scan_rebuild_incremental_unchanged(
    p: &ScanRebuildFilesParams<'_>,
    rel: &str,
    size: u64,
    mtime_ns: i64,
    sha_hex: &str,
    st: &mut ScanRebuildFileLoopState<'_>,
) -> bool {
    if p.incremental
        && !p.subtree
        && let Some((sz, mt, sh)) = p.catalog.get(rel)
        && *sz == size
        && *mt == mtime_ns
        && *sh == sha_hex
    {
        *st.files_unchanged += 1;
        return true;
    }
    false
}

#[cfg(feature = "fastembed")]
fn scan_rebuild_collect_file_chunks(
    rel: String,
    text: &str,
    ext: &str,
    p: &ScanRebuildFilesParams<'_>,
    st: &mut ScanRebuildFileLoopState<'_>,
) -> usize {
    let mut file_chunks = 0usize;
    for (sl, el, chunk) in chunk_text_lines(text, p.chunk_max_chars) {
        if chunk.chars().count() < 8 {
            continue;
        }
        let h = hash_chunk(&rel, &chunk);
        st.embed_batches
            .push((rel.clone(), h, sl, el, chunk, ext.to_string()));
        file_chunks += 1;
    }
    file_chunks
}

#[cfg(feature = "fastembed")]
fn scan_rebuild_finalize_chunks_outcome(
    p: &ScanRebuildFilesParams<'_>,
    rel: String,
    size: u64,
    mtime_ns: i64,
    sha_hex: String,
    file_chunks: usize,
    st: &mut ScanRebuildFileLoopState<'_>,
) -> Result<(), String> {
    if file_chunks > 0 {
        *st.files_indexed += 1;
        st.file_rows.push((rel, size, mtime_ns, sha_hex));
        return Ok(());
    }
    if p.incremental && !p.subtree {
        let _ = delete_rows_for_rel(
            p.tx,
            p.ws_key,
            rel.as_str(),
            "删除空块文件旧块失败",
            "删除空块文件目录行失败",
        );
    }
    *st.skipped_files = st.skipped_files.saturating_add(1);
    Ok(())
}

#[cfg(feature = "fastembed")]
fn scan_rebuild_process_one_file(
    p: &ScanRebuildFilesParams<'_>,
    path: &Path,
    rel: String,
    ext: &str,
    st: &mut ScanRebuildFileLoopState<'_>,
) -> Result<(), String> {
    let Some((size, mtime_ns, text, sha_hex)) = file_fingerprint(path, p.max_file_bytes) else {
        scan_rebuild_handle_missing_fingerprint(p, rel.as_str(), st);
        return Ok(());
    };
    if scan_rebuild_incremental_unchanged(p, rel.as_str(), size, mtime_ns, &sha_hex, st) {
        return Ok(());
    }
    if p.incremental && !p.subtree {
        delete_rows_for_rel(
            p.tx,
            p.ws_key,
            rel.as_str(),
            "删除旧块失败",
            "删除旧文件目录行失败",
        )?;
    }
    if *st.files_indexed >= p.rebuild_max_files {
        *st.skipped_files = st.skipped_files.saturating_add(1);
        return Ok(());
    }
    let file_chunks = scan_rebuild_collect_file_chunks(rel.clone(), &text, ext, p, st);
    scan_rebuild_finalize_chunks_outcome(p, rel, size, mtime_ns, sha_hex, file_chunks, st)
}

#[cfg(feature = "fastembed")]
fn scan_rebuild_files(p: ScanRebuildFilesParams<'_>) -> Result<RebuildScanOutcome, String> {
    let walker = WalkBuilder::new(p.search_root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();
    let mut files_indexed = 0usize;
    let mut files_unchanged = 0usize;
    let mut skipped_files = 0usize;
    let mut embed_batches: Vec<EmbedBatchRow> = Vec::new();
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
        if let Some(pat) = p.file_glob_pat
            && !pat.matches(&name)
        {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext.is_empty() || !p.ext_set.contains(&ext) {
            continue;
        }
        let Some(rel) = rel_path_for_workspace(p.ws_root, path) else {
            continue;
        };
        seen_rels.insert(rel.clone());
        let mut loop_state = ScanRebuildFileLoopState {
            files_indexed: &mut files_indexed,
            files_unchanged: &mut files_unchanged,
            skipped_files: &mut skipped_files,
            embed_batches: &mut embed_batches,
            file_rows: &mut file_rows,
        };
        scan_rebuild_process_one_file(&p, path, rel, ext.as_str(), &mut loop_state)?;
    }
    Ok(RebuildScanOutcome {
        files_indexed,
        files_unchanged,
        skipped_files,
        embed_batches,
        seen_rels,
        file_rows,
    })
}

#[cfg(feature = "fastembed")]
fn remove_stale_catalog_rows(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    catalog: &HashMap<String, (u64, i64, String)>,
    seen_rels: &HashSet<String>,
    incremental: bool,
    subtree: bool,
) -> Result<(), String> {
    if !incremental || subtree {
        return Ok(());
    }
    let stale: Vec<String> = catalog
        .keys()
        .filter(|k| !seen_rels.contains(*k))
        .cloned()
        .collect();
    for rel in stale {
        delete_rows_for_rel(
            tx,
            ws_key,
            rel.as_str(),
            "删除已删除文件的块失败",
            "删除文件目录行失败",
        )?;
    }
    Ok(())
}

#[cfg(feature = "fastembed")]
fn write_embedding_batches(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    embedder: &mut TextEmbedding,
    embed_batches: &[EmbedBatchRow],
) -> Result<usize, String> {
    const BATCH: usize = 32;
    let mut chunks_total = 0usize;
    let mut i = 0usize;
    while i < embed_batches.len() {
        let end = (i + BATCH).min(embed_batches.len());
        let docs: Vec<String> = embed_batches[i..end]
            .iter()
            .map(|(rel, _, _, _, body, ext)| embed_doc_for_chunk(rel, ext, body))
            .collect();
        let docs_ref: Vec<&str> = docs.iter().map(|s| s.as_str()).collect();
        let embeddings = embedder
            .embed(docs_ref, None)
            .map_err(|e| format!("嵌入批处理失败: {}", e))?;
        if embeddings.len() != end - i {
            return Err("嵌入批处理返回维度不一致".to_string());
        }
        for (j, emb) in embeddings.into_iter().enumerate() {
            let blob = f32_slice_to_bytes(&emb);
            let (rel, h, sl, el, body, _) = &embed_batches[i + j];
            tx.execute(
                &format!(
                    "INSERT INTO {TABLE} (workspace_root, rel_path, start_line, end_line, chunk_text, content_hash, embedding) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                ),
                params![ws_key, rel, *sl as i64, *el as i64, body, h, blob],
            )
            .map_err(|e| format!("写入索引失败: {}", e))?;
            chunks_total += 1;
        }
        i = end;
    }
    Ok(chunks_total)
}

#[cfg(feature = "fastembed")]
fn write_file_rows_and_meta(
    tx: &rusqlite::Transaction<'_>,
    ws_key: &str,
    file_rows: Vec<(String, u64, i64, String)>,
) -> Result<(), String> {
    for (rel, sz, mt, sha) in file_rows {
        tx.execute(
            &format!(
                "INSERT OR REPLACE INTO {TABLE_FILES} (workspace_root, rel_path, size, mtime_ns, content_sha256) VALUES (?1, ?2, ?3, ?4, ?5)"
            ),
            params![ws_key, rel, sz as i64, mt, sha],
        )
        .map_err(|e| format!("写入文件目录失败: {}", e))?;
    }
    tx.execute(
        "INSERT OR REPLACE INTO crabmate_codebase_index_meta (key, value) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )
    .map_err(|e| format!("写入元数据失败: {}", e))?;
    Ok(())
}

pub struct RebuildIndexParams<'a> {
    pub ws_root: &'a Path,
    pub ws_key: &'a str,
    pub index_path: &'a Path,
    pub sub_path: Option<&'a str>,
    pub max_file_bytes: usize,
    pub chunk_max_chars: usize,
    pub rebuild_max_files: usize,
    pub ext_set: &'a HashSet<String>,
    pub file_glob_pat: Option<&'a glob::Pattern>,
    pub incremental: bool,
}

pub fn rebuild_index(p: RebuildIndexParams<'_>) -> String {
    let RebuildIndexParams {
        ws_root,
        ws_key,
        index_path,
        sub_path,
        max_file_bytes,
        chunk_max_chars,
        rebuild_max_files,
        ext_set,
        file_glob_pat,
        incremental,
    } = p;
    #[cfg(not(feature = "fastembed"))]
    {
        return "错误：rebuild_index 需要本地向量嵌入；当前二进制未启用 `fastembed` Cargo feature。请使用带 fastembed 的构建，或关闭 codebase_semantic_search。".to_string();
    }

    #[cfg(feature = "fastembed")]
    {
        let mut conn = match open_codebase_semantic_db(index_path) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let search_root = match resolve_rebuild_search_root(ws_root, sub_path) {
            Ok(p) => p,
            Err(e) => return e,
        };

        let tx = match conn.transaction() {
            Ok(t) => t,
            Err(e) => return format!("索引事务开始失败: {}", e),
        };
        let subtree = match clear_rebuild_scope_rows(&tx, ws_key, sub_path, incremental) {
            Ok(v) => v,
            Err(e) => return e,
        };

        let mut embedder = match ensure_embedder() {
            Ok(m) => m,
            Err(e) => return e,
        };

        let catalog = match load_incremental_catalog(&tx, ws_key, incremental, subtree) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let scan = match scan_rebuild_files(ScanRebuildFilesParams {
            ws_root,
            ws_key,
            tx: &tx,
            search_root: &search_root,
            max_file_bytes,
            chunk_max_chars,
            rebuild_max_files,
            ext_set,
            file_glob_pat,
            incremental,
            subtree,
            catalog: &catalog,
        }) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let RebuildScanOutcome {
            files_indexed,
            files_unchanged,
            skipped_files,
            embed_batches,
            seen_rels,
            file_rows,
        } = scan;

        if let Err(e) =
            remove_stale_catalog_rows(&tx, ws_key, &catalog, &seen_rels, incremental, subtree)
        {
            return e;
        }

        let chunks_total = match write_embedding_batches(&tx, ws_key, &mut embedder, &embed_batches)
        {
            Ok(n) => n,
            Err(e) => return e,
        };
        if let Err(e) = write_file_rows_and_meta(&tx, ws_key, file_rows) {
            return e;
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
}
