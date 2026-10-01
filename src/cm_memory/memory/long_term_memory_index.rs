//! 长期记忆「回合后索引」：把本轮 user/assistant 分块写入 SQLite，并按嵌入配方补齐向量。
//!
//! 与 `long_term_memory` 同属一个运行时（`LongTermMemoryRuntime`），拆出独立文件以控制单文件行数。

#![cfg_attr(not(feature = "fastembed"), allow(dead_code))]

#[cfg(feature = "fastembed")]
use crate::cm_memory::memory::long_term_memory::f32_slice_to_bytes;
use crate::cm_memory::memory::long_term_memory::{
    LongTermMemoryRuntime, chunk_text, clamp_text, last_user_assistant_final_pair_for_turn,
};
use crate::cm_memory::memory::long_term_memory_store;
use crate::cm_config::{AgentConfig, LongTermMemoryVectorBackend};
use crate::cm_types::Message;

/// 回合索引写入的单条分块（正文 + 角色标识）。
type LongTermIndexTurnChunk = (String, &'static str);
/// [`LongTermMemoryRuntime::index_turn_chunks_to_store`] 的返回值。
type LongTermIndexTurnChunks =
    Result<Option<Vec<LongTermIndexTurnChunk>>, Box<dyn std::error::Error + Send + Sync>>;

impl LongTermMemoryRuntime {
    /// 索引本轮 user/assistant 分块，并把配方迁移后失效的历史向量逐步补回。
    pub(super) fn index_turn_blocking(
        rt: &LongTermMemoryRuntime,
        cfg: &AgentConfig,
        scope_id: &str,
        messages: &[Message],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let Some(to_store) = Self::index_turn_chunks_to_store(cfg, messages)? else {
            return Ok(());
        };
        let need_embed = Self::index_turn_needs_fastembed_embedding(cfg);
        Self::ensure_embedder_if_indexing(rt, need_embed)?;
        Self::store_index_chunks(rt, cfg, scope_id, to_store, need_embed)?;
        if need_embed {
            Self::backfill_missing_embeddings(rt, scope_id)?;
        }
        Ok(())
    }

    /// 写入本回合分块（同文本去重 + 超限淘汰）；`need_embed` 为真时逐条计算向量。
    fn store_index_chunks(
        rt: &LongTermMemoryRuntime,
        cfg: &AgentConfig,
        scope_id: &str,
        to_store: Vec<LongTermIndexTurnChunk>,
        need_embed: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
            let emb = Self::embed_auto_index_bytes_if_needed(rt, &text, need_embed)?;
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

    fn ensure_embedder_if_indexing(
        rt: &LongTermMemoryRuntime,
        need_embed: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if !need_embed {
            return Ok(());
        }
        #[cfg(feature = "fastembed")]
        LongTermMemoryRuntime::ensure_embedder(&rt.embedder)?;
        let _ = rt;
        Ok(())
    }

    fn embed_auto_index_bytes_if_needed(
        rt: &LongTermMemoryRuntime,
        text: &str,
        need_embed: bool,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        if !need_embed {
            return Ok(None);
        }
        #[cfg(feature = "fastembed")]
        {
            return Ok(Some(Self::embed_auto_index_chunk_bytes(rt, text)?));
        }
        #[cfg(not(feature = "fastembed"))]
        {
            let _ = (rt, text);
            Ok(None)
        }
    }

    /// 每回合最多回填的向量条数：嵌入是 CPU 密集操作，限额以免拖长后台回合处理。
    const EMBEDDING_BACKFILL_PER_TURN: usize = 8;

    #[cfg(feature = "fastembed")]
    /// 为「无向量」的旧行逐步补回向量（配额见 [`Self::EMBEDDING_BACKFILL_PER_TURN`]）。
    ///
    /// 配方迁移会清空全部历史向量；没有回填这些行就永远只能走关键词召回。
    /// 单条失败只跳过并计数，不影响其他行与主流程。
    fn backfill_missing_embeddings(
        rt: &LongTermMemoryRuntime,
        scope_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let pending = {
            let conn = rt
                .conn
                .lock()
                .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;
            long_term_memory_store::list_missing_embedding_for_scope(
                &conn,
                scope_id,
                Self::EMBEDDING_BACKFILL_PER_TURN,
            )?
        };
        if pending.is_empty() {
            return Ok(());
        }
        let total = pending.len();
        Self::ensure_embedder(&rt.embedder)?;
        let mut failed = 0usize;
        for (id, text) in pending {
            // 嵌入在释放 SQLite 锁之后执行，避免长事务阻塞其他读写。
            let emb = match Self::embed_auto_index_chunk_bytes(rt, &text) {
                Ok(e) => e,
                Err(e) => {
                    failed += 1;
                    log::debug!(target: "crabmate", "长期记忆向量回填失败 memory_id={id} error={e}");
                    continue;
                }
            };
            let conn = rt
                .conn
                .lock()
                .map_err(|e| format!("长期记忆 SQLite 锁失败: {e}"))?;
            long_term_memory_store::update_embedding(&conn, id, &emb)?;
        }
        log::debug!(
            target: "crabmate",
            "长期记忆向量回填：本回合处理 {total} 条，失败 {failed} 条"
        );
        Ok(())
    }

    #[cfg(not(feature = "fastembed"))]
    /// 未编译 `fastembed` feature 时无嵌入能力，回填为空操作。
    fn backfill_missing_embeddings(
        _rt: &LongTermMemoryRuntime,
        _scope_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    #[cfg(feature = "fastembed")]
    fn embed_auto_index_chunk_bytes(
        rt: &LongTermMemoryRuntime,
        text: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let mut g = rt
            .embedder
            .lock()
            .map_err(|e| format!("embedder 锁失败: {e}"))?;
        let model = g.as_mut().ok_or("embedder 未初始化")?;
        // 与查询侧同一配方：AllMiniLML6V2 不加 `query:` / `passage:` 前缀。
        let v = model
            .embed(vec![text], None)
            .map_err(|e| format!("嵌入失败: {e}"))?;
        let vec = v.into_iter().next().ok_or("嵌入结果为空")?;
        Ok(f32_slice_to_bytes(&vec))
    }
}
