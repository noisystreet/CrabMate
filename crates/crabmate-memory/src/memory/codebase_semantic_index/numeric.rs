//! 分块、哈希、余弦与 FTS 分数归一等纯算法辅助。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use regex::Regex;
use sha2::{Digest, Sha256};

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

/// 从 Rust 代码块提取符号名，供嵌入文本增强（与 `rebuild` 路径一致）。
pub fn rust_symbol_hints_for_chunk(chunk: &str) -> String {
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
            s = truncate_to_char_boundary(&s, MAX);
            s.push('…');
        }
        format!("symbols: {}", s)
    }
}

pub fn default_code_extensions() -> HashSet<&'static str> {
    [
        "rs", "toml", "md", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "java", "kt", "c",
        "cc", "cpp", "cxx", "h", "hpp", "json", "yaml", "yml", "sh", "bash", "css", "html", "vue",
        "svelte", "rb", "php", "swift", "scala", "clj", "ex", "exs", "erl", "hs", "ml", "mli",
        "fs", "fsi", "cs", "sql", "graphql", "proto",
    ]
    .into_iter()
    .collect()
}

pub fn chunk_text_lines(s: &str, max_chunk: usize) -> Vec<(usize, usize, String)> {
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
            if chunk.is_empty() && add_len > max_chunk {
                break;
            }
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
            push_long_line_chunks(&mut out, lines[i], i + 1, max_chunk);
            i += 1;
            continue;
        }
        out.push((start_line, end_i, chunk));
        i = end_i;
    }
    out
}

fn push_long_line_chunks(
    out: &mut Vec<(usize, usize, String)>,
    line: &str,
    line_no: usize,
    max_chunk: usize,
) {
    let mut start = 0usize;
    while start < line.len() {
        let end = next_safe_chunk_end(line, start, max_chunk);
        out.push((line_no, line_no, line[start..end].to_string()));
        start = end;
    }
}

fn next_safe_chunk_end(line: &str, start: usize, max_chunk: usize) -> usize {
    let mut end = (start + max_chunk).min(line.len());
    while end > start && !line.is_char_boundary(end) {
        end -= 1;
    }
    if end == start {
        line[start..]
            .chars()
            .next()
            .map(|c| start + c.len_utf8())
            .unwrap_or(line.len())
    } else {
        end
    }
}

pub fn hash_chunk(rel_path: &str, body: &str) -> String {
    let mut h = Sha256::new();
    h.update(rel_path.as_bytes());
    h.update(b"\0");
    h.update(body.as_bytes());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

pub fn cosine_sim(a: &[f32], b: &[f32]) -> f32 {
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

pub fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

pub fn bytes_to_f32_slice(blob: &[u8]) -> Option<Vec<f32>> {
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

#[cfg(feature = "fastembed")]
pub fn ensure_embedder() -> Result<TextEmbedding, String> {
    TextEmbedding::try_new(TextInitOptions::new(EmbeddingModel::AllMiniLML6V2))
        .map_err(|e| format!("fastembed 初始化失败: {}", e))
}

pub fn rel_path_for_workspace(workspace_root: &Path, file: &Path) -> Option<String> {
    let rel = file.strip_prefix(workspace_root).ok()?;
    let s = rel.to_string_lossy().replace('\\', "/");
    if s.is_empty() {
        Some(".".to_string())
    } else {
        Some(s)
    }
}

/// `rebuild_index` 的 `path`：用于 DELETE 的 POSIX 前缀（无首尾 `/`，`.` 或空视为整库）。
pub fn posix_subdir_prefix_for_delete(sub: &str) -> Option<String> {
    let s = sub.replace('\\', "/");
    let s = s.trim().trim_matches('/');
    if s.is_empty() || s == "." {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn sqlite_like_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            o.push('\\');
        }
        o.push(ch);
    }
    o
}

/// 将用户查询拆成若干词项，转为 FTS5 `MATCH` 安全表达式（词项 `AND`，词内双引号加倍）。
pub fn fts5_match_expression(query: &str) -> Option<String> {
    let parts: Vec<&str> = query.split_whitespace().filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return None;
    }
    let mut terms = Vec::with_capacity(parts.len());
    for p in parts {
        let escaped = p.replace('"', "\"\"");
        terms.push(format!("\"{escaped}\""));
    }
    Some(terms.join(" AND "))
}

pub fn norm_scores_bm25(scores: &[(i64, f64)]) -> HashMap<i64, f32> {
    if scores.is_empty() {
        return HashMap::new();
    }
    let mut min_v = f64::INFINITY;
    let mut max_v = f64::NEG_INFINITY;
    for (_, s) in scores {
        min_v = min_v.min(*s);
        max_v = max_v.max(*s);
    }
    let span = max_v - min_v;
    let mut m = HashMap::with_capacity(scores.len());
    if span.abs() < f64::EPSILON {
        let mid = 0.5f32;
        for (id, _) in scores {
            m.insert(*id, mid);
        }
    } else {
        for (id, s) in scores {
            let t = ((*s - min_v) / span).clamp(0.0, 1.0) as f32;
            m.insert(*id, t);
        }
    }
    m
}

/// UTF-8 安全的字节截断：在 `max_bytes` 以内找到最近的 char boundary 并截取。
fn truncate_to_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::chunk_text_lines;

    #[test]
    fn chunk_text_lines_splits_long_utf8_line_safely() {
        let chunks = chunk_text_lines("a你b", 2);

        assert_eq!(
            chunks
                .iter()
                .map(|(_, _, body)| body.as_str())
                .collect::<Vec<_>>(),
            vec!["a", "你", "b"]
        );
    }
}
