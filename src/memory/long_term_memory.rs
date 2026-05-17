//! 长期记忆：注入模型上下文（`prepare`）与回合结束后索引（`index_turn`）。
//!
//! - **作用域**：当前仅 `conversation_id`（与 `LongTermMemoryScopeMode::Conversation` 一致）。
//! - **安全**：索引前截断正文；日志不输出全文。无 Web 鉴权时勿依赖其隔离性（见 README）。

#![cfg_attr(not(feature = "fastembed"), allow(dead_code))]

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

#[cfg(feature = "fastembed")]
use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use log::{debug, info, warn};
use rusqlite::Connection;

use crate::config::{AgentConfig, LongTermMemoryVectorBackend};
use crate::memory::long_term_memory_store::{self, MemoryRow};
use crate::redact::preview_chars;
use crate::types::{
    CRABMATE_LONG_TERM_MEMORY_NAME, Message, is_chat_ui_separator, is_long_term_memory_injection,
    is_workspace_changelist_injection,
};

fn clamp_text(s: &str, max: usize) -> String {
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

fn chunk_text(s: &str, max_chunk: usize) -> Vec<String> {
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

fn f32_slice_to_bytes(v: &[f32]) -> Vec<u8> {
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

/// 回合索引写入的单条分块（正文 + 角色标识）。
type LongTermIndexTurnChunk = (String, &'static str);
/// [`LongTermMemoryRuntime::index_turn_chunks_to_store`] 的返回值。
type LongTermIndexTurnChunks =
    Result<Option<Vec<LongTermIndexTurnChunk>>, Box<dyn std::error::Error + Send + Sync>>;

/// 进程内共享：SQLite 连接 + 可选 fastembed（首次 embed 时初始化；未编译 **`fastembed`** feature 时无嵌入器）。
pub struct LongTermMemoryRuntime {
    conn: Arc<Mutex<Connection>>,
    #[cfg(feature = "fastembed")]
    embedder: Mutex<Option<TextEmbedding>>,
    #[cfg(not(feature = "fastembed"))]
    _no_fastembed: (),
    pub(crate) index_errors: AtomicU64,
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
    fn ensure_embedder(
        embedder: &Mutex<Option<TextEmbedding>>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut g = embedder
            .lock()
            .map_err(|e| format!("长期记忆 embedder 锁失败: {e}"))?;
        if g.is_none() {
            let model = TextEmbedding::try_new(TextInitOptions::new(EmbeddingModel::AllMiniLML6V2))
                .map_err(|e| format!("长期记忆 fastembed 初始化失败: {e}"))?;
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
        let prefixed = format!("passage: {}", passage);
        let v = model
            .embed(vec![prefixed], None)
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

    fn pick_ranked_memory_chunks(
        &self,
        cfg: &AgentConfig,
        rows: Vec<MemoryRow>,
        query: &str,
    ) -> Option<Vec<crate::memory::long_term_memory_recall::RecallPick>> {
        let top_k = cfg.long_term_memory.long_term_memory_top_k;
        let prioritize = cfg
            .long_term_memory
            .long_term_memory_prioritize_experience_recall;

        let vector_picked = match cfg.long_term_memory.long_term_memory_vector_backend {
            LongTermMemoryVectorBackend::Fastembed => {
                #[cfg(feature = "fastembed")]
                {
                    if let Err(e) = Self::ensure_embedder(&self.embedder) {
                        warn!(target: "crabmate", "长期记忆嵌入不可用，回退关键词检索: {}", e);
                        None
                    } else {
                        let q_emb = {
                            let mut g = self.embedder.lock().ok()?;
                            let model = (*g).as_mut()?;
                            let docs = vec![format!("query: {}", query)];
                            match model.embed(docs, None) {
                                Ok(v) => v.into_iter().next(),
                                Err(e) => {
                                    warn!(target: "crabmate", "长期记忆 query 嵌入失败: {}", e);
                                    None
                                }
                            }
                        };
                        let qv = q_emb?;
                        let mut scored: Vec<(f32, MemoryRow)> = Vec::with_capacity(rows.len());
                        for row in &rows {
                            let base = if let Some(ref b) = row.embedding {
                                bytes_to_f32_slice(b)
                                    .map(|ev| cosine_sim(&qv, &ev))
                                    .unwrap_or(0.0)
                            } else {
                                0.0
                            };
                            let score = crate::memory::long_term_memory_recall::score_row(
                                base, row, query, prioritize,
                            );
                            scored.push((score, row.clone()));
                        }
                        Some(crate::memory::long_term_memory_recall::pick_recall_chunks(
                            top_k, query, scored, prioritize,
                        ))
                    }
                }
                #[cfg(not(feature = "fastembed"))]
                {
                    warn!(
                        target: "crabmate",
                        "长期记忆向量后端为 fastembed 但本构建未启用 `fastembed` feature，回退关键词检索"
                    );
                    None
                }
            }
            LongTermMemoryVectorBackend::Disabled
            | LongTermMemoryVectorBackend::Qdrant
            | LongTermMemoryVectorBackend::Pgvector => {
                if matches!(
                    cfg.long_term_memory.long_term_memory_vector_backend,
                    LongTermMemoryVectorBackend::Qdrant | LongTermMemoryVectorBackend::Pgvector
                ) {
                    debug!(target: "crabmate", "长期记忆向量后端 {:?} 未接外部服务，回退关键词检索", cfg.long_term_memory.long_term_memory_vector_backend);
                }
                None
            }
        };

        let picked = vector_picked.unwrap_or_else(|| {
            crate::memory::long_term_memory_recall::keyword_rank_rows(
                top_k, rows, query, prioritize,
            )
        });
        if picked.is_empty() {
            None
        } else {
            Some(picked)
        }
    }

    fn format_ltm_injection_body(
        picked: &[crate::memory::long_term_memory_recall::RecallPick],
        budget: usize,
    ) -> Option<String> {
        let mut body = String::from(
            "以下为与当前问题可能相关的长期记忆（【经验 #id】为可复用提炼；[记忆 #id] 为回合摘要；可用 long_term_memory_list 核对；若无关请忽略）：\n\n",
        );
        let mut used = 0usize;
        for (_score, id, t, role) in picked.iter() {
            if used >= budget {
                break;
            }
            let entry = crate::memory::long_term_memory_recall::format_recall_entry(*id, role, t);
            if used + entry.len() > budget {
                let remain = budget.saturating_sub(used);
                if remain > 8 {
                    body.push_str(&preview_chars(&entry, remain));
                }
                break;
            }
            body.push_str(&entry);
            used += entry.len();
        }
        if body.len() < 80 {
            return None;
        }
        Some(body)
    }

    #[cfg(feature = "fastembed")]
    fn embed_auto_index_chunk_bytes(
        rt: &LongTermMemoryRuntime,
        text: &str,
        role: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let mut g = rt
            .embedder
            .lock()
            .map_err(|e| format!("embedder 锁失败: {e}"))?;
        let model = g.as_mut().ok_or("embedder 未初始化")?;
        let prefixed = if role == "user" {
            format!("query: {}", text)
        } else {
            format!("passage: {}", text)
        };
        let v = model
            .embed(vec![prefixed], None)
            .map_err(|e| format!("嵌入失败: {e}"))?;
        let vec = v.into_iter().next().ok_or("嵌入结果为空")?;
        Ok(f32_slice_to_bytes(&vec))
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
            crate::memory::auto_summarize_experience::draft_auto_experience_from_turn(messages)
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

    fn index_turn_blocking(
        rt: &LongTermMemoryRuntime,
        cfg: &AgentConfig,
        scope_id: &str,
        messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(to_store) = Self::index_turn_chunks_to_store(cfg, messages)? else {
            return Ok(());
        };
        let need_embed = Self::index_turn_needs_fastembed_embedding(cfg);
        if need_embed {
            #[cfg(feature = "fastembed")]
            LongTermMemoryRuntime::ensure_embedder(&rt.embedder)?;
        }
        let conn = rt
            .conn
            .lock()
            .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        let auto_expires = (cfg.long_term_memory.long_term_memory_default_ttl_secs > 0)
            .then_some(now + cfg.long_term_memory.long_term_memory_default_ttl_secs as i64);
        for (text, role) in to_store {
            if long_term_memory_store::has_duplicate_text(&conn, scope_id, &text)? {
                continue;
            }
            let emb = if need_embed {
                #[cfg(feature = "fastembed")]
                {
                    Some(Self::embed_auto_index_chunk_bytes(rt, &text, role)?)
                }
                #[cfg(not(feature = "fastembed"))]
                {
                    None::<Vec<u8>>
                }
            } else {
                None
            };
            long_term_memory_store::insert_chunk(
                &conn,
                scope_id,
                &text,
                role,
                auto_expires,
                emb.as_deref(),
            )?;
            long_term_memory_store::delete_oldest_beyond(
                &conn,
                scope_id,
                cfg.long_term_memory.long_term_memory_max_entries,
            )?;
        }
        Ok(())
    }

    /// 若本回合无需写入长期记忆，返回 `Ok(None)`。
    fn index_turn_chunks_to_store(
        cfg: &AgentConfig,
        messages: &[Message],
    ) -> LongTermIndexTurnChunks {
        if !cfg.long_term_memory.long_term_memory_auto_index_turns {
            return Ok(None);
        }
        let Some((user_t, asst_t)) = last_user_assistant_final_pair_for_turn(messages) else {
            return Ok(None);
        };
        let user_t = clamp_text(
            user_t,
            cfg.long_term_memory.long_term_memory_max_chars_per_chunk,
        );
        let asst_t = clamp_text(
            asst_t,
            cfg.long_term_memory.long_term_memory_max_chars_per_chunk,
        );
        if user_t.len() < cfg.long_term_memory.long_term_memory_min_chars_to_index
            && asst_t.len() < cfg.long_term_memory.long_term_memory_min_chars_to_index
        {
            return Ok(None);
        }
        let max = cfg.long_term_memory.long_term_memory_max_chars_per_chunk;
        let mut to_store: Vec<LongTermIndexTurnChunk> = Vec::new();
        for part in chunk_text(&user_t, max) {
            to_store.push((part, "user"));
        }
        for part in chunk_text(&asst_t, max) {
            to_store.push((part, "assistant"));
        }
        Ok(Some(to_store))
    }

    fn index_turn_needs_fastembed_embedding(cfg: &AgentConfig) -> bool {
        cfg!(feature = "fastembed")
            && matches!(
                cfg.long_term_memory.long_term_memory_vector_backend,
                LongTermMemoryVectorBackend::Fastembed
            )
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
        let c = crate::types::message_content_as_str(&m.content)?.trim();
        if c.is_empty() {
            continue;
        }
        return Some(c);
    }
    None
}

/// 最后一轮「用户提问 → 助手终答」（无 `tool_calls`）的正文对。
pub(crate) fn last_user_assistant_final_pair_for_turn(
    messages: &[Message],
) -> Option<(&str, &str)> {
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
        let ac = crate::types::message_content_as_str(&m.content)?.trim();
        if ac.is_empty() {
            continue;
        }
        let mut j = i;
        while j > 0 {
            j -= 1;
            let u = &messages[j];
            if u.role != "user" {
                continue;
            }
            if is_long_term_memory_injection(u) {
                continue;
            }
            let uc = crate::types::message_content_as_str(&u.content)?.trim();
            if uc.is_empty() {
                continue;
            }
            return Some((uc, ac));
        }
        return None;
    }
    None
}

/// 为 `ToolContext` 准备长期记忆运行时与会话 id；未启用或缺运行时返回 `(None, None)`。
pub(crate) fn tool_context_memory_extras(
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
