//! 代码语义索引的查询路径：FTS / 向量混合检索与结果格式化。

use std::cmp::Ordering;
#[cfg(feature = "fastembed")]
use std::cmp::Reverse;
#[cfg(feature = "fastembed")]
use std::collections::BinaryHeap;
use std::collections::HashMap;
use std::path::Path;

use rusqlite::{Connection, OptionalExtension, params};

use super::{TABLE, TABLE_FTS, fts5_match_expression, norm_scores_bm25, open_codebase_semantic_db};
#[cfg(feature = "fastembed")]
use super::{bytes_to_f32_slice, cosine_sim, ensure_embedder};

#[derive(Clone)]
struct ScoredChunk {
    id: i64,
    score: f32,
    cosine: f32,
    fts: f32,
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
            .then_with(|| self.id.cmp(&other.id))
    }
}

impl PartialOrd for ScoredChunk {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RetrieveMode {
    Hybrid,
    SemanticOnly,
    FtsOnly,
}

impl RetrieveMode {
    pub(super) fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "hybrid" => Ok(Self::Hybrid),
            "semantic_only" => Ok(Self::SemanticOnly),
            "fts_only" => Ok(Self::FtsOnly),
            _ => Err(format!(
                "错误：retrieve_mode 须为 hybrid、semantic_only 或 fts_only，收到 {:?}",
                s
            )),
        }
    }
}

/// `search_index` 的查询侧参数（与 `run_tool` 解析结果对应）。
pub(super) struct SearchQueryParams {
    pub(super) top_k: usize,
    pub(super) query_max_chunks: usize,
    pub(super) max_out_chars: usize,
    pub(super) mode: RetrieveMode,
    pub(super) fts_top_n: usize,
    pub(super) hybrid_semantic_pool: usize,
    pub(super) hybrid_alpha: f32,
}

/// `format_search_output` 的元信息（避免过多函数参数）。
struct SearchOutputHeader {
    mode: RetrieveMode,
    top_k: usize,
    hybrid_alpha: f32,
    fts_rows_fetched: usize,
    vec_scanned: usize,
    limit_active: bool,
    query_max_chunks: usize,
    max_out_chars: usize,
}

fn format_search_output(header: &SearchOutputHeader, scored: &[ScoredChunk]) -> String {
    let limit_note = if header.limit_active {
        format!("，上限 {}", header.query_max_chunks)
    } else {
        String::new()
    };
    let approx_note = if header.limit_active && header.vec_scanned >= header.query_max_chunks {
        "（已达向量扫描上限，语义分支为近似）"
    } else {
        ""
    };

    let mode_zh = match header.mode {
        RetrieveMode::SemanticOnly => "semantic_only（仅向量）",
        RetrieveMode::FtsOnly => "fts_only（仅 FTS 全文）",
        RetrieveMode::Hybrid => "hybrid（FTS BM25 + 向量余弦加权）",
    };

    let mut out = String::new();
    out.push_str(&format!(
        "代码检索 mode={}，top_k={}，hybrid_alpha={:.2}，FTS 候选 {} 条，向量已扫描 {} 块{}{}。\n\
         hybrid 综合分 = α×cosine + (1-α)×fts_norm；fts_only 按 BM25 归一化排序。\n\n",
        mode_zh,
        header.top_k,
        header.hybrid_alpha,
        header.fts_rows_fetched,
        header.vec_scanned,
        limit_note,
        approx_note,
    ));
    let mut used = 0usize;
    for (rank, chunk) in scored.iter().enumerate() {
        let line_hdr = if header.mode == RetrieveMode::Hybrid {
            format!(
                "## {}. {} (行 {}–{})  hybrid={:.4}  cos={:.4}  fts={:.4}\n",
                rank + 1,
                chunk.rel,
                chunk.sl,
                chunk.el,
                chunk.score,
                chunk.cosine,
                chunk.fts
            )
        } else if header.mode == RetrieveMode::FtsOnly {
            format!(
                "## {}. {} (行 {}–{})  fts={:.4}\n",
                rank + 1,
                chunk.rel,
                chunk.sl,
                chunk.el,
                chunk.score
            )
        } else {
            format!(
                "## {}. {} (行 {}–{})  cos={:.4}\n",
                rank + 1,
                chunk.rel,
                chunk.sl,
                chunk.el,
                chunk.cosine
            )
        };
        let fence = "```\n";
        let footer = "```\n\n";
        let budget = header.max_out_chars.saturating_sub(used);
        if budget < line_hdr.len() + fence.len() + footer.len() + 20 {
            out.push_str("\n… 输出已达长度上限，后续结果已省略 …\n");
            break;
        }
        let remain = budget - line_hdr.len() - fence.len() - footer.len();
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
        out.push_str(&line_hdr);
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

/// `fts_only`：仅按 BM25 命中块排序输出（不跑向量）。
fn search_index_fts_only_branch(
    conn: &Connection,
    ws_key: &str,
    fts_by_id: &HashMap<i64, f32>,
    q: &SearchQueryParams,
    fts_rows_fetched: usize,
) -> String {
    if fts_by_id.is_empty() {
        return format!(
            "fts_only：当前查询无 FTS 命中（workspace_root={}）。可换关键词、使用 hybrid，或确认已 rebuild_index（schema 含 FTS5）。",
            ws_key
        );
    }
    let mut ids: Vec<i64> = fts_by_id.keys().copied().collect();
    ids.sort_unstable();
    let mut scored = Vec::with_capacity(ids.len());
    let sql_one = format!(
        "SELECT rel_path, start_line, end_line, chunk_text FROM {TABLE} WHERE workspace_root = ?1 AND id = ?2"
    );
    for id in ids {
        let fts_n = *fts_by_id.get(&id).unwrap_or(&0.0);
        let row: Option<(String, i64, i64, String)> = conn
            .query_row(&sql_one, params![ws_key, id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()
            .ok()
            .flatten();
        let Some((rel, sl, el, text)) = row else {
            continue;
        };
        scored.push(ScoredChunk {
            id,
            score: fts_n,
            cosine: 0.0,
            fts: fts_n,
            rel,
            sl,
            el,
            text,
        });
    }
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
    let scored: Vec<ScoredChunk> = scored.into_iter().take(q.top_k).collect();
    if scored.is_empty() {
        return format!(
            "索引中无匹配条目（workspace_root={}）。请先使用 rebuild_index=true 构建索引。",
            ws_key
        );
    }
    let hdr = SearchOutputHeader {
        mode: q.mode,
        top_k: q.top_k,
        hybrid_alpha: q.hybrid_alpha,
        fts_rows_fetched,
        vec_scanned: 0,
        limit_active: false,
        query_max_chunks: 0,
        max_out_chars: q.max_out_chars,
    };
    format_search_output(&hdr, &scored)
}

#[cfg(feature = "fastembed")]
fn semantic_search_embed_query_vector(query: &str) -> Result<Vec<f32>, String> {
    let mut embedder = ensure_embedder()?;
    let q_emb = embedder
        .embed(vec![format!("query: {}", query)], None)
        .map_err(|e| format!("查询嵌入失败: {}", e))?;
    q_emb
        .into_iter()
        .next()
        .ok_or_else(|| "查询嵌入失败: 空结果".to_string())
}

#[cfg(feature = "fastembed")]
fn semantic_heap_offer_top_k(
    heap: &mut BinaryHeap<Reverse<ScoredChunk>>,
    pool_k: usize,
    item: ScoredChunk,
) {
    if heap.len() < pool_k {
        heap.push(Reverse(item));
    } else if let Some(Reverse(worst)) = heap.peek()
        && item.score > worst.score
    {
        heap.pop();
        heap.push(Reverse(item));
    }
}

#[cfg(feature = "fastembed")]
fn semantic_fastembed_pool_k(q: &SearchQueryParams) -> usize {
    if q.mode == RetrieveMode::Hybrid {
        q.hybrid_semantic_pool.max(q.top_k)
    } else {
        q.top_k
    }
}

#[cfg(feature = "fastembed")]
struct SemanticChunkRow {
    id: i64,
    rel: String,
    sl: i64,
    el: i64,
    text: String,
    blob: Vec<u8>,
}

#[cfg(feature = "fastembed")]
fn semantic_fastembed_scored_chunk(
    qv: &[f32],
    row: SemanticChunkRow,
    q: &SearchQueryParams,
    fts_by_id: &HashMap<i64, f32>,
) -> ScoredChunk {
    let SemanticChunkRow {
        id,
        rel,
        sl,
        el,
        text,
        blob,
    } = row;
    let cosine = bytes_to_f32_slice(&blob)
        .map(|ev| cosine_sim(qv, &ev))
        .unwrap_or(0.0);
    let fts_n = *fts_by_id.get(&id).unwrap_or(&0.0);
    let (score, fts_for_row) = if q.mode == RetrieveMode::SemanticOnly {
        (cosine, 0.0f32)
    } else {
        let s = q.hybrid_alpha * cosine + (1.0 - q.hybrid_alpha) * fts_n;
        (s, fts_n)
    };
    ScoredChunk {
        id,
        score,
        cosine,
        fts: fts_for_row,
        rel,
        sl,
        el,
        text,
    }
}

#[cfg(feature = "fastembed")]
fn semantic_fastembed_scan_into_heap(
    conn: &Connection,
    ws_key: &str,
    qv: &[f32],
    q: &SearchQueryParams,
    fts_by_id: &HashMap<i64, f32>,
    pool_k: usize,
) -> Result<(usize, BinaryHeap<Reverse<ScoredChunk>>), String> {
    let mut stmt = conn
        .prepare_cached(&format!(
            "SELECT id, rel_path, start_line, end_line, chunk_text, embedding FROM {TABLE} WHERE workspace_root = ?1"
        ))
        .map_err(|e| format!("读取索引失败: {}", e))?;
    let rows = stmt
        .query_map(params![ws_key], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Vec<u8>>(5)?,
            ))
        })
        .map_err(|e| format!("遍历索引失败: {}", e))?;
    let mut heap: BinaryHeap<Reverse<ScoredChunk>> = BinaryHeap::new();
    let mut scanned = 0usize;
    let limit_active = q.query_max_chunks > 0;
    for row in rows {
        if limit_active && scanned >= q.query_max_chunks {
            break;
        }
        let (id, rel, sl, el, text, blob) = match row {
            Ok(x) => x,
            Err(_) => continue,
        };
        scanned = scanned.saturating_add(1);
        let item = semantic_fastembed_scored_chunk(
            qv,
            SemanticChunkRow {
                id,
                rel,
                sl,
                el,
                text,
                blob,
            },
            q,
            fts_by_id,
        );
        semantic_heap_offer_top_k(&mut heap, pool_k, item);
    }
    Ok((scanned, heap))
}

#[cfg(feature = "fastembed")]
fn search_index_semantic_fastembed(
    conn: &Connection,
    ws_key: &str,
    query: &str,
    q: &SearchQueryParams,
    fts_by_id: &HashMap<i64, f32>,
    fts_rows_fetched: usize,
) -> String {
    let pool_k = semantic_fastembed_pool_k(q);
    let qv = match semantic_search_embed_query_vector(query) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let (scanned, heap) =
        match semantic_fastembed_scan_into_heap(conn, ws_key, &qv, q, fts_by_id, pool_k) {
            Ok(x) => x,
            Err(e) => return e,
        };
    let mut pool: Vec<ScoredChunk> = heap.into_iter().map(|r| r.0).collect();
    pool.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(Ordering::Equal));
    let scored: Vec<ScoredChunk> = pool.into_iter().take(q.top_k).collect();
    if scored.is_empty() {
        return format!(
            "索引中无匹配条目（workspace_root={}）。请先使用 rebuild_index=true 构建索引；若为 fts_only 且无分词命中，可换关键词或改用 hybrid/semantic_only。",
            ws_key
        );
    }
    let limit_active = q.query_max_chunks > 0;
    let hdr = SearchOutputHeader {
        mode: q.mode,
        top_k: q.top_k,
        hybrid_alpha: q.hybrid_alpha,
        fts_rows_fetched,
        vec_scanned: scanned,
        limit_active,
        query_max_chunks: q.query_max_chunks,
        max_out_chars: q.max_out_chars,
    };
    format_search_output(&hdr, &scored)
}

pub(super) fn search_index(
    ws_key: &str,
    index_path: &Path,
    query: &str,
    q: SearchQueryParams,
) -> String {
    let conn = match open_codebase_semantic_db(index_path) {
        Ok(c) => c,
        Err(e) => return e,
    };

    #[cfg(not(feature = "fastembed"))]
    if matches!(q.mode, RetrieveMode::SemanticOnly) {
        return "错误：retrieve_mode=semantic_only 需要本地向量嵌入；当前二进制未启用 `fastembed` Cargo feature。请改用 fts_only，或使用带 fastembed 的构建。".to_string();
    }

    let mode = {
        #[cfg(not(feature = "fastembed"))]
        {
            if matches!(q.mode, RetrieveMode::Hybrid) {
                RetrieveMode::FtsOnly
            } else {
                q.mode
            }
        }
        #[cfg(feature = "fastembed")]
        {
            q.mode
        }
    };

    // ── FTS 候选（BM25）────────────────────────────────────────
    let mut fts_by_id: HashMap<i64, f32> = HashMap::new();
    let mut fts_rows_fetched = 0usize;
    if mode != RetrieveMode::SemanticOnly
        && let Some(fts_q) = fts5_match_expression(query)
    {
        let sql = format!(
            "SELECT c.id, bm25({TABLE_FTS}) AS rank \
             FROM {TABLE_FTS} f \
             JOIN {TABLE} c ON c.id = f.rowid \
             WHERE c.workspace_root = ?1 AND f.chunk_text MATCH ?2 \
             ORDER BY rank ASC LIMIT ?3"
        );
        if let Ok(mut stmt) = conn.prepare(&sql)
            && let Ok(it) = stmt.query_map(params![ws_key, fts_q, q.fts_top_n as i64], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, f64>(1)?))
            })
        {
            let mut raw: Vec<(i64, f64)> = Vec::new();
            for row in it.flatten() {
                raw.push(row);
            }
            fts_rows_fetched = raw.len();
            fts_by_id = norm_scores_bm25(&raw);
        }
    }

    // ── fts_only：仅按 FTS 命中的块 id 拉取，不跑向量 ────────────
    if mode == RetrieveMode::FtsOnly {
        return search_index_fts_only_branch(&conn, ws_key, &fts_by_id, &q, fts_rows_fetched);
    }

    #[cfg(not(feature = "fastembed"))]
    {
        return "（内部错误）search_index：无 fastembed 时不应到达此分支".to_string();
    }

    #[cfg(feature = "fastembed")]
    {
        search_index_semantic_fastembed(&conn, ws_key, query, &q, &fts_by_id, fts_rows_fetched)
    }
}
