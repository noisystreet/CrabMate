//! 长期记忆：注入模型上下文（`prepare`）与回合结束后索引（`index_turn`）。
//!
//! - **作用域**：当前仅 `conversation_id`（与 `LongTermMemoryScopeMode::Conversation` 一致）。
//! - **安全**：索引前截断正文；日志不输出全文。无 Web 鉴权时勿依赖其隔离性（见 README）。

use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};

use fastembed::{EmbeddingModel, TextEmbedding, TextInitOptions};
use log::{debug, warn};
use rusqlite::Connection;

use crate::config::{AgentConfig, LongTermMemoryVectorBackend};
use crate::long_term_memory_store;
use crate::redact::preview_chars;
use crate::types::{
    CRABMATE_LONG_TERM_MEMORY_NAME, Message, is_chat_ui_separator, is_long_term_memory_injection,
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

/// 进程内共享：SQLite 连接 + 可选 fastembed（首次 embed 时初始化）。
pub struct LongTermMemoryRuntime {
    conn: Arc<Mutex<Connection>>,
    embedder: Mutex<Option<TextEmbedding>>,
    pub(crate) index_errors: AtomicU64,
}

impl LongTermMemoryRuntime {
    pub fn open(path: &Path) -> Result<Arc<Self>, Box<dyn std::error::Error + Send + Sync>> {
        let conn = long_term_memory_store::open_file(path)?;
        Ok(Arc::new(Self {
            conn: Arc::new(Mutex::new(conn)),
            embedder: Mutex::new(None),
            index_errors: AtomicU64::new(0),
        }))
    }

    /// 与会话库共用已打开的 SQLite 连接（表已由 `open_conversation_sqlite` 迁移）。
    pub fn new_shared_sqlite(conn: Arc<Mutex<Connection>>) -> Arc<Self> {
        Arc::new(Self {
            conn,
            embedder: Mutex::new(None),
            index_errors: AtomicU64::new(0),
        })
    }

    /// 与会话库共用同一文件时，仅迁移表（连接已由 `open_conversation_sqlite` 打开）。
    pub fn migrate_on_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
        long_term_memory_store::migrate(conn)
    }

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

    /// 在 `prepare_messages_for_model` 之前调用：按配置注入一条 `user`（或跳过）。
    pub fn prepare_messages(
        self: &Arc<Self>,
        cfg: &AgentConfig,
        scope_id: Option<&str>,
        messages: &mut Vec<Message>,
    ) {
        if !cfg.long_term_memory_enabled {
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
                cfg.long_term_memory_max_entries,
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

        let mut picked: Vec<(f32, String)> = Vec::new();
        match cfg.long_term_memory_vector_backend {
            LongTermMemoryVectorBackend::Fastembed => {
                if let Err(e) = Self::ensure_embedder(&self.embedder) {
                    warn!(target: "crabmate", "长期记忆嵌入不可用，跳过向量检索: {}", e);
                    for (_, text, _, _, _) in rows.iter().take(cfg.long_term_memory_top_k) {
                        picked.push((0.0, text.clone()));
                    }
                } else {
                    let q_emb = {
                        let mut g = match self.embedder.lock() {
                            Ok(x) => x,
                            Err(_) => return,
                        };
                        let Some(ref mut model) = *g else {
                            return;
                        };
                        let docs = vec![format!("query: {}", q)];
                        match model.embed(docs, None) {
                            Ok(v) => v.into_iter().next(),
                            Err(e) => {
                                warn!(target: "crabmate", "长期记忆 query 嵌入失败: {}", e);
                                None
                            }
                        }
                    };
                    let Some(qv) = q_emb else {
                        return;
                    };
                    for (_id, text, _role, _ts, emb_blob) in rows {
                        let score = if let Some(ref b) = emb_blob {
                            bytes_to_f32_slice(b)
                                .map(|ev| cosine_sim(&qv, &ev))
                                .unwrap_or(0.0)
                        } else {
                            0.0
                        };
                        picked.push((score, text));
                    }
                    picked
                        .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
                    picked.truncate(cfg.long_term_memory_top_k);
                }
            }
            LongTermMemoryVectorBackend::Disabled
            | LongTermMemoryVectorBackend::Qdrant
            | LongTermMemoryVectorBackend::Pgvector => {
                if matches!(
                    cfg.long_term_memory_vector_backend,
                    LongTermMemoryVectorBackend::Qdrant | LongTermMemoryVectorBackend::Pgvector
                ) {
                    debug!(target: "crabmate", "长期记忆向量后端 {:?} 未接外部服务，按时间倒序取用", cfg.long_term_memory_vector_backend);
                }
                for (_, text, _, _, _) in rows.iter().take(cfg.long_term_memory_top_k) {
                    picked.push((0.0, text.clone()));
                }
            }
        }

        let mut body = String::from(
            "以下为与当前问题可能相关的历史摘要（来自本会话长期记忆，供参考；若无关请忽略）：\n\n",
        );
        let mut used = 0usize;
        let budget = cfg.long_term_memory_inject_max_chars;
        for (i, (_s, t)) in picked.iter().enumerate() {
            if used >= budget {
                break;
            }
            let entry = format!("{}. {}\n\n", i + 1, t);
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
            return;
        }

        let insert_at = system_insert_index(messages);
        messages.insert(
            insert_at,
            Message {
                role: "user".to_string(),
                content: Some(body),
                reasoning_content: None,
                tool_calls: None,
                name: Some(CRABMATE_LONG_TERM_MEMORY_NAME.to_string()),
                tool_call_id: None,
            },
        );
    }

    /// 回合成功结束后异步索引本轮 user/assistant（不含 tool 正文）。
    pub fn spawn_index_turn(
        self: Arc<Self>,
        cfg: Arc<AgentConfig>,
        scope_id: String,
        messages: Vec<Message>,
    ) {
        if !cfg.long_term_memory_enabled {
            return;
        }
        if scope_id.trim().is_empty() {
            return;
        }
        if !cfg.long_term_memory_async_index {
            return;
        }
        tokio::spawn(async move {
            if let Err(e) = Self::index_turn_blocking(&self, &cfg, &scope_id, &messages) {
                self.index_errors
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                warn!(
                    target: "crabmate",
                    "长期记忆索引失败 scope_len={} error={}",
                    scope_id.len(),
                    e
                );
            }
        });
    }

    fn index_turn_blocking(
        rt: &LongTermMemoryRuntime,
        cfg: &AgentConfig,
        scope_id: &str,
        messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some((user_t, asst_t)) = last_user_assistant_final_pair(messages) else {
            return Ok(());
        };
        let user_t = clamp_text(user_t, cfg.long_term_memory_max_chars_per_chunk);
        let asst_t = clamp_text(asst_t, cfg.long_term_memory_max_chars_per_chunk);
        if user_t.len() < cfg.long_term_memory_min_chars_to_index
            && asst_t.len() < cfg.long_term_memory_min_chars_to_index
        {
            return Ok(());
        }

        let mut to_store: Vec<(String, &'static str)> = Vec::new();
        for part in chunk_text(&user_t, cfg.long_term_memory_max_chars_per_chunk) {
            to_store.push((part, "user"));
        }
        for part in chunk_text(&asst_t, cfg.long_term_memory_max_chars_per_chunk) {
            to_store.push((part, "assistant"));
        }

        let need_embed = matches!(
            cfg.long_term_memory_vector_backend,
            LongTermMemoryVectorBackend::Fastembed
        );
        if need_embed {
            LongTermMemoryRuntime::ensure_embedder(&rt.embedder)?;
        }

        let conn = rt
            .conn
            .lock()
            .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;

        for (text, role) in to_store {
            if long_term_memory_store::has_duplicate_text(&conn, scope_id, &text)? {
                continue;
            }
            let emb = if need_embed {
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
                Some(f32_slice_to_bytes(&vec))
            } else {
                None
            };
            long_term_memory_store::insert_chunk(&conn, scope_id, &text, role, emb.as_deref())?;
            long_term_memory_store::delete_oldest_beyond(
                &conn,
                scope_id,
                cfg.long_term_memory_max_entries,
            )?;
        }
        Ok(())
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
        if is_long_term_memory_injection(m) {
            continue;
        }
        let c = m.content.as_deref()?.trim();
        if c.is_empty() {
            continue;
        }
        return Some(c);
    }
    None
}

/// 最后一轮「用户提问 → 助手终答」（无 `tool_calls`）的正文对。
fn last_user_assistant_final_pair(messages: &[Message]) -> Option<(&str, &str)> {
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
        let ac = m.content.as_deref()?.trim();
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
            let uc = u.content.as_deref()?.trim();
            if uc.is_empty() {
                continue;
            }
            return Some((uc, ac));
        }
        return None;
    }
    None
}

static CLI_MEMORY_RUNTIME: OnceLock<Arc<LongTermMemoryRuntime>> = OnceLock::new();

/// CLI / 单进程：在首次需要时打开 `path` 并缓存。
pub fn cli_runtime_lazy(
    path: &Path,
) -> Result<Arc<LongTermMemoryRuntime>, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(r) = CLI_MEMORY_RUNTIME.get() {
        return Ok(Arc::clone(r));
    }
    let r = LongTermMemoryRuntime::open(path)?;
    let _ = CLI_MEMORY_RUNTIME.set(Arc::clone(&r));
    Ok(r)
}
