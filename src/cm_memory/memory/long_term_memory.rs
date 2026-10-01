//! 长期记忆：注入模型上下文（`prepare`）与显式读写（`long_term_remember` / `long_term_forget` / `long_term_memory_list`）。
//!
//! - **作用域**：当前仅 `conversation_id`。
//! - **安全**：索引前截断正文；日志不输出全文。无 Web 鉴权时勿依赖其隔离性（见 README）。
//! - 回合结束后索引见 `long_term_memory_index`。

#![cfg_attr(not(feature = "fastembed"), allow(dead_code))]

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

#[cfg(feature = "fastembed")]
use fastembed::TextEmbedding;
use log::{debug, info, warn};
use rusqlite::Connection;

use crate::cm_memory::memory::long_term_memory_recall as recall;
use crate::cm_memory::memory::long_term_memory_store::{self, MemoryRow};
use crate::cm_config::{AgentConfig, LongTermMemoryVectorBackend};
use crate::cm_types::text_utils::preview_chars;
use crate::cm_types::{
    CRABMATE_LONG_TERM_MEMORY_NAME, Message, is_chat_ui_separator, is_long_term_memory_injection,
    is_workspace_changelist_injection,
};

pub(super) fn clamp_text(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let t = s.trim();
    if t.chars().count() <= max {
        return t.to_string();
    }
    let mut out = String::new();
    for ch in t.chars().take(max) {
        out.push(ch);
    }
    out.push('…');
    out
}

pub(super) fn chunk_text(s: &str, max_chunk: usize) -> Vec<String> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    if max_chunk == 0 || s.len() <= max_chunk {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    for part in s.split("\n\n") {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if p.chars().count() <= max_chunk {
            out.push(p.to_string());
        } else {
            let mut start = 0;
            let chars: Vec<char> = p.chars().collect();
            while start < chars.len() {
                let end = (start + max_chunk).min(chars.len());
                out.push(chars[start..end].iter().collect());
                start = end;
            }
        }
    }
    if out.is_empty() {
        out.push(clamp_text(s, max_chunk));
    }
    out
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

pub(super) fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut b = Vec::with_capacity(v.len() * 4);
    for x in v {
        b.extend_from_slice(&x.to_le_bytes());
    }
    b
}

fn bytes_to_f32_slice(b: &[u8]) -> Option<Vec<f32>> {
    if !b.len().is_multiple_of(4) {
        return None;
    }
    let n = b.len() / 4;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let chunk = b.get(i * 4..i * 4 + 4)?;
        let arr: [u8; 4] = chunk.try_into().ok()?;
        out.push(f32::from_le_bytes(arr));
    }
    Some(out)
}

fn validate_explicit_remember_scope_and_text<'a>(
    cfg: &AgentConfig,
    scope_id: &'a str,
    text: &str,
) -> Result<(&'a str, String), String> {
    if !cfg.long_term_memory.long_term_memory_enabled {
        return Err("长期记忆未启用（long_term_memory_enabled = false）".to_string());
    }
    let scope = scope_id.trim();
    if scope.is_empty() {
        return Err("长期记忆作用域为空（无会话 id）".to_string());
    }
    let text = clamp_text(
        text,
        cfg.long_term_memory.long_term_memory_max_chars_per_chunk,
    );
    if text.is_empty() {
        return Err("记忆正文为空".to_string());
    }
    Ok((scope, text))
}

/// 进程内共享：SQLite 连接 + 可选 fastembed（首次 embed 时初始化；未编译 **`fastembed`** feature 时无嵌入器）。
pub struct LongTermMemoryRuntime {
    pub(super) conn: Arc<Mutex<Connection>>,
    #[cfg(feature = "fastembed")]
    pub(super) embedder: Mutex<Option<TextEmbedding>>,
    #[cfg(not(feature = "fastembed"))]
    _no_fastembed: (),
    pub index_errors: AtomicU64,
}

impl LongTermMemoryRuntime {
    pub fn open(path: &Path) -> Result<Arc<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let conn = long_term_memory_store::open_file(path)?;
        Ok(Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
            #[cfg(feature = "fastembed")]
            embedder: Mutex::new(None),
            #[cfg(not(feature = "fastembed"))]
            _no_fastembed: (),
            index_errors: AtomicU64::new(0),
        }))
    }

    /// 与会话库共用已打开的 SQLite 连接（表已由 `open_conversation_sqlite` 迁移）。
    pub fn new_shared_sqlite(conn: Arc<Mutex<Connection>>) -> Arc<Self> {
        Arc::new(Self {
            conn,
            #[cfg(feature = "fastembed")]
            embedder: Mutex::new(None),
            #[cfg(not(feature = "fastembed"))]
            _no_fastembed: (),
            index_errors: AtomicU64::new(0),
        })
    }

    /// 与会话库共用同一文件时，仅迁移表（连接已由 `open_conversation_sqlite` 打开）。
    pub fn migrate_on_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
        long_term_memory_store::migrate(conn)
    }

    #[cfg(feature = "fastembed")]
    pub(super) fn ensure_embedder(
        embedder: &Mutex<Option<TextEmbedding>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut g = embedder
            .lock()
            .map_err(|e| format!("长期记忆 embedder 锁失败: {e}"))?;
        if g.is_none() {
            let model = crate::cm_memory::memory::fastembed_init::try_new_text_embedding()
                .map_err(|e| format!("长期记忆 {e}"))?;
            *g = Some(model);
        }
        Ok(())
    }

    #[cfg(feature = "fastembed")]
    fn explicit_chunk_embedding_bytes(
        &self,
        cfg: &AgentConfig,
        passage: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        let need_embed = matches!(
            cfg.long_term_memory.long_term_memory_vector_backend,
            LongTermMemoryVectorBackend::Fastembed
        );
        if !need_embed {
            return Ok(None);
        }
        Self::ensure_embedder(&self.embedder).map_err(|e| e.to_string())?;
        let mut g = self
            .embedder
            .lock()
            .map_err(|e| format!("embedder 锁失败: {e}"))?;
        let model = g.as_mut().ok_or_else(|| "embedder 未初始化".to_string())?;
        // AllMiniLML6V2 是普通 sentence-transformer，不加 E5 式前缀（前缀只会引入噪声）。
        let v = model
            .embed(vec![passage], None)
            .map_err(|e| format!("嵌入失败: {e}"))?;
        let vec = v
            .into_iter()
            .next()
            .ok_or_else(|| "嵌入结果为空".to_string())?;
        Ok(Some(f32_slice_to_bytes(&vec)))
    }

    #[cfg(not(feature = "fastembed"))]
    fn explicit_chunk_embedding_bytes(
        &self,
        _cfg: &AgentConfig,
        _passage: &str,
    ) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }

    /// 在 `prepare_messages_for_model` 之前调用：按配置注入一条 `user`（或跳过）。
    pub fn prepare_messages(
        self: &Arc<Self>,
        cfg: &AgentConfig,
        scope_id: Option<&str>,
        messages: &mut Vec<Message>,
    ) {
        if !cfg.long_term_memory.long_term_memory_enabled {
            return;
        }
        let Some(scope) = scope_id.filter(|s| !s.trim().is_empty()) else {
            return;
        };
        messages.retain(|m| !is_long_term_memory_injection(m));

        let query = last_user_query_for_memory(messages);
        let Some(q) = query else {
            return;
        };
        let q = clamp_text(q, 4096);
        if q.is_empty() {
            return;
        }

        let rows = {
            let g = match self.conn.lock() {
                Ok(x) => x,
                Err(_) => return,
            };
            match long_term_memory_store::list_for_scope(
                &g,
                scope,
                cfg.long_term_memory.long_term_memory_max_entries,
            ) {
                Ok(r) => r,
                Err(e) => {
                    warn!(target: "crabmate", "长期记忆读取失败 scope_len={} error={}", scope.len(), e);
                    return;
                }
            }
        };

        if rows.is_empty() {
            return;
        }

        let Some(picked) = self.pick_ranked_memory_chunks(cfg, rows, &q) else {
            return;
        };

        let Some(body) = Self::format_ltm_injection_body(
            picked.as_slice(),
            cfg.long_term_memory.long_term_memory_inject_max_chars,
        ) else {
            return;
        };

        let insert_at = system_insert_index(messages);
        messages.insert(
            insert_at,
            Message {
                role: "user".to_string(),
                content: Some(body.into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some(CRABMATE_LONG_TERM_MEMORY_NAME.to_string()),
                tool_call_id: None,
            },
        );
    }

    fn warn_unwired_vector_backend(backend: LongTermMemoryVectorBackend) {
        if matches!(
            backend,
            LongTermMemoryVectorBackend::Qdrant | LongTermMemoryVectorBackend::Pgvector
        ) {
            debug!(
                target: "crabmate",
                "长期记忆向量后端 {:?} 未接外部服务，回退关键词检索",
                backend
            );
        }
    }

    /// 对候选行打「base + boost」排序分，返回 `(候选, 是否出现关键词回退 base)`。
    ///
    /// 第二个返回值供调用方决定能否做乘性相对衰减：一旦集合里混入关键词 base
    /// （量纲是 `命中词数 / 查询词总数`，与连续余弦不可比），乘性阈值就不可靠，
    /// 只能整体放弃裁剪——保守不裁优于误裁，见 [`recall::apply_relative_cutoff`]。
    fn score_memory_rows_against_query(
        rows: &[MemoryRow],
        qv: &[f32],
        query: &str,
        prioritize: bool,
    ) -> (Vec<recall::ScoredRow>, bool) {
        let mut scored = Vec::with_capacity(rows.len());
        let mut keyword_fallback = false;
        for row in rows {
            let (base, from_keywords) = match row
                .embedding
                .as_ref()
                .and_then(|b| bytes_to_f32_slice(b))
            {
                Some(ev) => {
                    let b = cosine_sim(qv, &ev);
                    // 阈值须判 base：boost 是加性的，用总分判定会让无关经验条靠 boost 越过门槛。
                    if b < recall::MIN_RECALL_BASE_SCORE {
                        continue;
                    }
                    (b, false)
                }
                // 无向量（旧数据被配方迁移作废 / 嵌入失败）的行回退关键词打分；
                // 否则这些行在向量路径下会被整体丢弃，关键词路径才是它们唯一的召回机会。
                None => {
                    let kw = recall::keyword_overlap_score(query, &row.chunk_text);
                    if kw <= 0.0 && !recall::tag_hit_in_query(row, query) {
                        continue;
                    }
                    (kw, true)
                }
            };
            keyword_fallback |= from_keywords;
            let score = recall::score_row(base, row, query, prioritize, from_keywords);
            scored.push((score, base, row.clone()));
        }
        (scored, keyword_fallback)
    }

    #[cfg(feature = "fastembed")]
    fn embed_memory_query_vector(&self, query: &str) -> Option<Vec<f32>> {
        if let Err(e) = Self::ensure_embedder(&self.embedder) {
            warn!(target: "crabmate", "长期记忆嵌入不可用，回退关键词检索: {}", e);
            return None;
        }
        let mut g = self.embedder.lock().ok()?;
        let model = (*g).as_mut()?;
        let docs = vec![query];
        match model.embed(docs, None) {
            Ok(v) => v.into_iter().next(),
            Err(e) => {
                warn!(target: "crabmate", "长期记忆 query 嵌入失败: {}", e);
                None
            }
        }
    }

    fn try_vector_pick_fastembed(
        &self,
        rows: &[MemoryRow],
        query: &str,
        top_k: usize,
        prioritize: bool,
    ) -> Option<Vec<recall::RecallPick>> {
        #[cfg(feature = "fastembed")]
        {
            let qv = self.embed_memory_query_vector(query)?;
            let (scored, keyword_fallback) =
                Self::score_memory_rows_against_query(rows, &qv, query, prioritize);
            // 纯余弦集（量纲稳定）才做乘性相对衰减剪长尾；混入关键词回退 base 就整体不裁。
            let cutoff = (!keyword_fallback).then_some(recall::RELATIVE_SCORE_RATIO);
            Some(recall::pick_recall_chunks(
                top_k, query, scored, prioritize, cutoff,
            ))
        }
        #[cfg(not(feature = "fastembed"))]
        {
            let _ = (self, rows, query, top_k, prioritize);
            warn!(
                target: "crabmate",
                "长期记忆向量后端为 fastembed 但本构建未启用 `fastembed` feature，回退关键词检索"
            );
            None
        }
    }

    fn pick_ranked_memory_chunks(
        &self,
        cfg: &AgentConfig,
        rows: Vec<MemoryRow>,
        query: &str,
    ) -> Option<Vec<recall::RecallPick>> {
        let top_k = cfg.long_term_memory.long_term_memory_top_k;
        let prioritize = cfg
            .long_term_memory
            .long_term_memory_prioritize_experience_recall;
        let backend = cfg.long_term_memory.long_term_memory_vector_backend;
        let total_rows = rows.len();
        let vector_picked = match backend {
            LongTermMemoryVectorBackend::Fastembed => {
                self.try_vector_pick_fastembed(&rows, query, top_k, prioritize)
            }
            LongTermMemoryVectorBackend::Disabled
            | LongTermMemoryVectorBackend::Qdrant
            | LongTermMemoryVectorBackend::Pgvector => {
                Self::warn_unwired_vector_backend(backend);
                None
            }
        };

        let used_vector = vector_picked.is_some();
        let picked = vector_picked
            .unwrap_or_else(|| recall::keyword_rank_rows(top_k, rows, query, prioritize));
        // 供实测校准 MIN_RECALL_BASE_SCORE / RELATIVE_SCORE_RATIO：观察真实会话的分数分布。
        debug!(
            target: "crabmate",
            "长期记忆召回：候选 {} 条，命中 {} 条（top_k={}，top1={:.3}，路径={}）",
            total_rows,
            picked.len(),
            top_k,
            picked.first().map(|p| p.0).unwrap_or(0.0),
            if used_vector { "vector" } else { "keyword" },
        );
        if picked.is_empty() {
            None
        } else {
            Some(picked)
        }
    }

    fn format_ltm_injection_body(picked: &[recall::RecallPick], budget: usize) -> Option<String> {
        let mut body = String::from(
            "以下为与当前问题可能相关的长期记忆（【经验 #id】为可复用提炼；[记忆 #id] 为回合摘要；可用 long_term_memory_list 核对；若无关请忽略）：\n\n",
        );
        let mut used = 0usize;
        for (_score, id, t, role) in picked.iter() {
            if used >= budget {
                break;
            }
            let entry = recall::format_recall_entry(*id, role, t);
            // 预算按字符计：用字节（`.len()`）比对会让中文实际注入量只有配额约 1/3。
            let entry_chars = entry.chars().count();
            if used + entry_chars > budget {
                let remain = budget.saturating_sub(used);
                if remain > 8 {
                    body.push_str(&preview_chars(&entry, remain));
                }
                break;
            }
            body.push_str(&entry);
            used += entry_chars;
        }
        if body.chars().count() < 80 {
            return None;
        }
        Some(body)
    }

    /// 回合成功结束后异步索引本轮 user/assistant，并按配置尝试自动沉淀经验。
    pub fn spawn_turn_memory_postprocess(
        self: Arc<Self>,
        cfg: Arc<AgentConfig>,
        scope_id: String,
        messages: Vec<Message>,
    ) {
        if !cfg.long_term_memory.long_term_memory_enabled {
            return;
        }
        if scope_id.trim().is_empty() {
            return;
        }
        if !cfg.long_term_memory.long_term_memory_async_index {
            return;
        }
        tokio::spawn(async move {
            let rt = Arc::clone(&self);
            if let Err(e) = Self::turn_memory_postprocess_blocking(rt, &cfg, &scope_id, &messages) {
                self.index_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                warn!(
                    target: "crabmate",
                    "长期记忆回合后处理失败 scope_len={} error={}",
                    scope_id.len(),
                    e
                );
            }
        });
    }

    fn turn_memory_postprocess_blocking(
        rt: Arc<LongTermMemoryRuntime>,
        cfg: &AgentConfig,
        scope_id: &str,
        messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Self::index_turn_blocking(rt.as_ref(), cfg, scope_id, messages)?;
        if cfg
            .long_term_memory
            .long_term_memory_auto_summarize_experience
        {
            Self::auto_summarize_experience_blocking(Arc::clone(&rt), cfg, scope_id, messages)?;
        }
        Ok(())
    }

    fn auto_summarize_experience_blocking(
        rt: Arc<LongTermMemoryRuntime>,
        cfg: &AgentConfig,
        scope_id: &str,
        messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some((experience, tags)) =
            crate::cm_memory::memory::auto_summarize_experience::draft_auto_experience_from_turn(messages)
        else {
            return Ok(());
        };
        match rt.summarize_experience_remember_blocking(
            cfg,
            scope_id,
            &experience,
            &tags,
            None,
            "auto_summarize_experience",
        ) {
            Ok(id) => {
                info!(
                    target: "crabmate",
                    "长期记忆：已自动沉淀经验 memory_id={id} scope_len={}",
                    scope_id.len()
                );
            }
            Err(e) => {
                debug!(
                    target: "crabmate",
                    "长期记忆：自动沉淀跳过或失败 error={e}"
                );
            }
        }
        Ok(())
    }

    /// 显式写入长期记忆（工具 `long_term_remember`）；`ttl_secs` 为 `None` 表示永不过期（仍受条数上限淘汰）。
    pub fn explicit_remember_blocking(
        self: &Arc<Self>,
        cfg: &AgentConfig,
        scope_id: &str,
        text: &str,
        tags: &[String],
        ttl_secs: Option<u64>,
    ) -> Result<i64, String> {
        self.summarize_experience_remember_blocking(cfg, scope_id, text, tags, ttl_secs, "explicit")
    }

    /// 经验写入（`summarize_experience` 工具或回合后自动沉淀）；`source_role` 区分来源。
    pub fn summarize_experience_remember_blocking(
        self: &Arc<Self>,
        cfg: &AgentConfig,
        scope_id: &str,
        text: &str,
        tags: &[String],
        ttl_secs: Option<u64>,
        source_role: &str,
    ) -> Result<i64, String> {
        let (scope, text) = validate_explicit_remember_scope_and_text(cfg, scope_id, text)?;
        let tags_json = serde_json::to_string(tags).map_err(|e| format!("tags 序列化失败: {e}"))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let expires_at = ttl_secs.map(|s| now + s as i64);
        let emb = self.explicit_chunk_embedding_bytes(cfg, text.as_str())?;

        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;
        if long_term_memory_store::has_duplicate_text(&conn, scope, &text)
            .map_err(|e| format!("去重检查失败: {e}"))?
        {
            return Err("与已有记忆正文重复，已跳过写入".to_string());
        }

        let id = long_term_memory_store::insert_explicit_chunk(
            &conn,
            scope,
            &text,
            source_role,
            &tags_json,
            expires_at,
            emb.as_deref(),
        )
        .map_err(|e| format!("写入长期记忆失败: {e}"))?;
        long_term_memory_store::delete_oldest_beyond(
            &conn,
            scope,
            cfg.long_term_memory.long_term_memory_max_entries,
        )
        .map_err(|e| format!("长期记忆淘汰失败: {e}"))?;
        Ok(id)
    }

    /// 按 id 或正文删除（工具 `long_term_forget`）。
    pub fn explicit_forget_blocking(
        &self,
        cfg: &AgentConfig,
        scope_id: &str,
        id: Option<i64>,
        text: Option<&str>,
        explicit_only: bool,
    ) -> Result<usize, String> {
        if !cfg.long_term_memory.long_term_memory_enabled {
            return Err("长期记忆未启用".to_string());
        }
        let scope = scope_id.trim();
        if scope.is_empty() {
            return Err("长期记忆作用域为空".to_string());
        }
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;
        if let Some(i) = id {
            return long_term_memory_store::delete_by_id_for_scope(&conn, scope, i)
                .map_err(|e| format!("删除失败: {e}"));
        }
        let Some(t) = text.map(str::trim).filter(|s| !s.is_empty()) else {
            return Err("须提供 memory_id 或 memory_text 之一".to_string());
        };
        long_term_memory_store::delete_matching_text(&conn, scope, t, explicit_only)
            .map_err(|e| format!("删除失败: {e}"))
    }

    /// 列出最近记忆条目（工具 `long_term_memory_list`）。
    pub fn list_recent_blocking(
        &self,
        cfg: &AgentConfig,
        scope_id: &str,
        limit: usize,
    ) -> Result<String, String> {
        if !cfg.long_term_memory.long_term_memory_enabled {
            return Err("长期记忆未启用".to_string());
        }
        let scope = scope_id.trim();
        if scope.is_empty() {
            return Err("长期记忆作用域为空".to_string());
        }
        let lim = limit.clamp(1, 64);
        let conn = self
            .conn
            .lock()
            .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;
        let rows = long_term_memory_store::list_recent_for_scope(&conn, scope, lim)
            .map_err(|e| format!("读取失败: {e}"))?;
        if rows.is_empty() {
            return Ok("（当前作用域无未过期记忆条目）".to_string());
        }
        let mut out =
            String::from("以下为长期记忆条目（从新到旧；expires 为 Unix 秒，空表示不过期）：\n\n");
        for (i, (id, text, kind, exp, tags)) in rows.iter().enumerate() {
            let exp_s = exp
                .map(|x| x.to_string())
                .unwrap_or_else(|| "—".to_string());
            let preview = preview_chars(text, 200);
            out.push_str(&format!(
                "{}. id={} kind={} expires={} tags={}\n   {}\n\n",
                i + 1,
                id,
                kind,
                exp_s,
                tags,
                preview
            ));
        }
        Ok(out)
    }
}

/// 从持久化/返回给客户端的消息列表中移除注入条（避免写入会话存储）。
pub fn strip_long_term_memory_injections(messages: &mut Vec<Message>) {
    messages.retain(|m| !is_long_term_memory_injection(m));
}

fn system_insert_index(messages: &[Message]) -> usize {
    let mut i = 0;
    while i < messages.len() && messages[i].role == "system" {
        i += 1;
    }
    i
}

fn last_user_query_for_memory(messages: &[Message]) -> Option<&str> {
    for m in messages.iter().rev() {
        if m.role != "user" {
            continue;
        }
        if is_long_term_memory_injection(m) || is_workspace_changelist_injection(m) {
            continue;
        }
        let c = crate::cm_types::message_content_as_str(&m.content)?.trim();
        if c.is_empty() {
            continue;
        }
        return Some(c);
    }
    None
}

fn preceding_user_text(messages: &[Message], before: usize) -> Option<&str> {
    let mut j = before;
    while j > 0 {
        j -= 1;
        let u = &messages[j];
        if u.role != "user" {
            continue;
        }
        if is_long_term_memory_injection(u) {
            continue;
        }
        let uc = crate::cm_types::message_content_as_str(&u.content)?.trim();
        if uc.is_empty() {
            continue;
        }
        return Some(uc);
    }
    None
}

/// 最后一轮「用户提问 → 助手终答」（无 `tool_calls`）的正文对。
pub fn last_user_assistant_final_pair_for_turn(messages: &[Message]) -> Option<(&str, &str)> {
    let mut i = messages.len();
    while i > 0 {
        i -= 1;
        let m = &messages[i];
        if m.role != "assistant" || m.tool_calls.is_some() {
            continue;
        }
        if is_chat_ui_separator(m) {
            continue;
        }
        let ac = crate::cm_types::message_content_as_str(&m.content)?.trim();
        if ac.is_empty() {
            continue;
        }
        let uc = preceding_user_text(messages, i)?;
        return Some((uc, ac));
    }
    None
}

/// 为 `ToolContext` 准备长期记忆运行时与会话 id；未启用或缺运行时返回 `(None, None)`。
pub fn tool_context_memory_extras(
    cfg: &AgentConfig,
    ltm: Option<Arc<LongTermMemoryRuntime>>,
    scope_id: Option<&str>,
) -> (Option<Arc<LongTermMemoryRuntime>>, Option<String>) {
    if !cfg.long_term_memory.long_term_memory_enabled {
        return (None, None);
    }
    let Some(rt) = ltm else {
        return (None, None);
    };
    let scope = scope_id
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(std::string::ToString::to_string);
    (Some(rt), scope)
}
