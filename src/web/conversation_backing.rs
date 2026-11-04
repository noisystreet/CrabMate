//! Web 会话存储门面：内存 / SQLite 两个后端的**唯一**分派点。
//!
//! S1-a（#1278）：原先 `AppStateConversationRuntime` 的每个方法都各自写一遍
//! `match &*backing { Memory => …, Sqlite => … }`，两套后端语义没有可对照的地方；
//! 同一段「`spawn_blocking` + 加锁 + 打日志」也复制了七八份（另一份在
//! `app_state_facets::sqlite_conversation_store_op_result`）。
//!
//! 这里把分派与 SQLite 错误收敛收拢成 [`ConversationBacking`] 的方法，runtime 只做薄委托。
//! 行为与搬移前逐字等价：内存后端不持久化归档、SQLite 分支的日志文案与 fallback 口径均未改。
//!
//! S1-b 将在这个门面上抽 `SessionStore` trait，并把 SQLite 实现从 `web` feature 解绑。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::cm_api_contract::chat::ConversationLayoutMeta;
use crate::conversation_store::{self, BranchTruncateOutcome, SaveConversationOutcome};
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::types::Message;

use crate::web::app_state::ConversationTurnSeed;

/// 内存后端单条会话（与 SQLite 行同语义）。
#[derive(Clone)]
pub(crate) struct MemoryConversationEntry {
    messages: Vec<Message>,
    /// 当前多角色工作台选用的命名角色 id；`None` 表示默认人格（与 Web 未持久化选用一致）。
    active_agent_role: Option<String>,
    /// 当前会话工作模式；`None` 表示未显式设置（回落配置默认）。
    active_session_mode: Option<String>,
    /// 当前会话权限等级**意图**；`None` 表示未显式设置（回落配置默认）。
    active_permission_level: Option<String>,
    /// B2：与 SQLite `layout_meta_json` 同语义，每次按当前 `messages` 派生写入。
    layout: Option<ConversationLayoutMeta>,
    revision: u64,
    updated_at: std::time::Instant,
}

impl MemoryConversationEntry {
    /// 按「`None` = 保持原值」把三列 `active_*` 意图写进条目（与 SQLite 侧 `set_active_state` 同语义）。
    ///
    /// 回合结束的整行覆盖**不能**拿旧快照回写：期间 `POST /conversation/state` 可能已经改过这些列。
    pub(crate) fn apply_active_state(
        &mut self,
        role: Option<&str>,
        mode: Option<&str>,
        level: Option<&str>,
    ) {
        for (slot, raw) in [
            (&mut self.active_agent_role, role),
            (&mut self.active_session_mode, mode),
            (&mut self.active_permission_level, level),
        ] {
            if let Some(v) = conversation_store::active_state_column(raw) {
                *slot = Some(v.to_string());
            }
        }
    }
}

/// 持久化列里「空串等价于未设置」的归一化（SQLite 侧存的是 `TEXT`，读回来是 `String`）。
fn nonempty_persisted_column(raw: String) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 会话保留与归档回收策略快照（一次读锁内取齐，见设计文档 §3.8 / §4.2）。
///
/// `Copy` 是刻意的：SQLite 分支要把它移进 `spawn_blocking` 闭包，
/// 传引用会要求 `&ConversationRetention` 活到 `'static`（编译器 E0521）。
#[derive(Clone, Copy)]
pub(crate) struct ConversationRetention {
    /// `0` = 不过期。
    pub(crate) ttl_secs: u64,
    /// `0` = 不限条数。
    pub(crate) max_entries: usize,
    /// 单会话归档行数上限；`0` = 不限（§4.2 / C4）。
    pub(crate) archive_max_rows_per_conversation: usize,
    /// 归档 codec 选择阈值（字节，§3.8）。
    pub(crate) archive_codec_min_bytes: usize,
}

/// 在 `spawn_blocking` 里跑一段同步 SQLite 操作，把三类失败统一成 `Err(String)`：
/// 锁中毒 / SQLite 报错 / 任务崩。
///
/// 调用方用 `unwrap_or` 退回 fallback（不区分「业务上的否」与「存储层失败」），
/// 或用 `?` 把失败上抛（需要区分两者时，如 `POST /conversation/state`）。
pub(crate) async fn sqlite_conversation_store_op<T>(
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
    id_log: String,
    op_zh: &'static str,
    run: impl FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let g = conn
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        run(&g).map_err(|e: rusqlite::Error| e.to_string())
    })
    .await
    {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(e)) => {
            log::error!(
                target: "crabmate",
                "会话 SQLite {}失败 conversation_id={} error={}",
                op_zh,
                id_log,
                e
            );
            Err(e)
        }
        Err(e) => {
            log::error!(
                target: "crabmate",
                "会话 SQLite {}任务失败 conversation_id={} error={}",
                op_zh,
                id_log,
                e
            );
            Err(e.to_string())
        }
    }
}

/// Web 会话存储后端。
///
/// **唯一分派点**：内存与 SQLite 的差异只出现在本 impl 内；上层
/// [`super::AppStateConversationRuntime`] 全是薄委托。
#[derive(Clone)]
pub(crate) enum ConversationBacking {
    Memory(Arc<tokio::sync::RwLock<HashMap<String, MemoryConversationEntry>>>),
    Sqlite(Arc<std::sync::Mutex<rusqlite::Connection>>),
}

impl ConversationBacking {
    pub(crate) fn memory_default() -> Self {
        Self::Memory(Arc::new(tokio::sync::RwLock::new(HashMap::new())))
    }

    pub(crate) fn is_sqlite(&self) -> bool {
        matches!(self, Self::Sqlite(_))
    }

    /// 按策略清理过期 / 超量会话（`serve` 启动时与低频定时任务调用）。
    pub(crate) async fn prune(&self, r: ConversationRetention) {
        match self {
            Self::Memory(map) => {
                let mut guard = map.write().await;
                Self::prune_memory_locked(
                    &mut guard,
                    std::time::Instant::now(),
                    r.ttl_secs,
                    r.max_entries,
                );
            }
            Self::Sqlite(conn) => {
                let c = Arc::clone(conn);
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(g) = c.lock() {
                        let _ = conversation_store::prune(
                            &g,
                            r.ttl_secs,
                            r.max_entries,
                            r.archive_max_rows_per_conversation,
                        );
                    }
                })
                .await;
            }
        }
    }

    /// 读会话种子：`messages` + revision + 三列 `active_*` + layout。
    ///
    /// 内存后端按 `ttl_secs` 惰性淘汰过期条目；SQLite 把 TTL 交给 `conversation_store::load`。
    pub(crate) async fn load_seed(
        &self,
        conversation_id: &str,
        ttl_secs: u64,
    ) -> Option<ConversationTurnSeed> {
        match self {
            Self::Memory(map) => {
                let mut guard = map.write().await;
                let entry = guard.get_mut(conversation_id)?;
                if ttl_secs > 0 && entry.updated_at.elapsed() > Duration::from_secs(ttl_secs) {
                    guard.remove(conversation_id);
                    return None;
                }
                entry.updated_at = std::time::Instant::now();
                Some(ConversationTurnSeed {
                    messages: entry.messages.clone(),
                    expected_revision: Some(entry.revision),
                    persisted_active_agent_role: entry.active_agent_role.clone(),
                    persisted_active_session_mode: entry.active_session_mode.clone(),
                    persisted_active_permission_level: entry.active_permission_level.clone(),
                    layout: entry.layout.clone(),
                })
            }
            Self::Sqlite(conn) => {
                let id = conversation_id.to_string();
                let c = Arc::clone(conn);
                let loaded = tokio::task::spawn_blocking(move || {
                    let g = match c.lock() {
                        Ok(g) => g,
                        Err(e) => {
                            log::error!(
                                target: "crabmate",
                                "会话 SQLite 锁失败: {}",
                                e
                            );
                            return None;
                        }
                    };
                    match conversation_store::load(&g, &id, ttl_secs) {
                        Ok(o) => o,
                        Err(e) => {
                            log::warn!(
                                target: "crabmate",
                                "会话 SQLite 读取失败 id={} error={}",
                                id,
                                e
                            );
                            None
                        }
                    }
                })
                .await
                .ok()
                .flatten();
                loaded.map(|row| ConversationTurnSeed {
                    messages: row.messages,
                    expected_revision: Some(row.revision),
                    persisted_active_agent_role: nonempty_persisted_column(row.active_agent_role),
                    persisted_active_session_mode: nonempty_persisted_column(
                        row.active_session_mode,
                    ),
                    persisted_active_permission_level: nonempty_persisted_column(
                        row.active_permission_level,
                    ),
                    layout: row.layout,
                })
            }
        }
    }

    /// 读取会话归档原文（`GET /conversation/messages` 带 `include_archive=true`，见设计文档 §5.3.4 C2）。
    ///
    /// 仅 SQLite 后端持久化归档；内存后端无归档，返回空。锁 / 读取 / 解码失败记录日志并返回空，
    /// 不阻断快照响应（与 `list_all_messages_json` 对归档失败的容错口径一致）。
    pub(crate) async fn load_archive_messages(&self, conversation_id: &str) -> Vec<Message> {
        let Self::Sqlite(conn) = self else {
            return Vec::new();
        };
        let id = conversation_id.to_string();
        let c = Arc::clone(conn);
        tokio::task::spawn_blocking(move || {
            let g = match c.lock() {
                Ok(g) => g,
                Err(e) => {
                    log::error!(target: "crabmate", "会话归档 SQLite 锁失败: {}", e);
                    return Vec::new();
                }
            };
            match conversation_store::load_archive_messages(&g, &id) {
                Ok(messages) => messages,
                Err(e) => {
                    log::warn!(
                        target: "crabmate",
                        "会话归档读取失败 id={} error={}",
                        id,
                        e
                    );
                    Vec::new()
                }
            }
        })
        .await
        .unwrap_or_default()
    }

    /// 整行覆盖 + CAS（`expected_revision`）+ 归档行落盘 + 保留策略回收。
    ///
    /// `None` 传参表示「保持原值」——回合结束的落盘不该覆盖期间 `POST /conversation/state` 的写入。
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn save_if_revision(
        &self,
        conversation_id: String,
        messages: Vec<Message>,
        active_agent_role: Option<&str>,
        active_session_mode: Option<&str>,
        active_permission_level: Option<&str>,
        expected_revision: Option<u64>,
        archive_rows: Vec<conversation_store::ArchiveRow>,
        r: ConversationRetention,
    ) -> SaveConversationOutcome {
        match self {
            Self::Memory(map) => {
                let mut guard = map.write().await;
                let now = std::time::Instant::now();
                // `None` = 保持原值：回合结束的落盘不该覆盖期间 `POST /conversation/state` 的写入。
                let role_owned =
                    conversation_store::active_state_column(active_agent_role).map(str::to_string);
                let mode_owned =
                    conversation_store::active_state_column(active_session_mode).map(str::to_string);
                let level_owned = conversation_store::active_state_column(active_permission_level)
                    .map(str::to_string);
                if let Some(entry) = guard.get_mut(&conversation_id) {
                    match expected_revision {
                        Some(exp) if entry.revision == exp => {
                            entry.layout = Some(crate::cm_turn_layout::layout_meta_from_messages(
                                &messages,
                            ));
                            entry.messages = messages;
                            entry.apply_active_state(
                                role_owned.as_deref(),
                                mode_owned.as_deref(),
                                level_owned.as_deref(),
                            );
                            entry.revision = entry.revision.saturating_add(1);
                            entry.updated_at = now;
                        }
                        _ => return SaveConversationOutcome::Conflict,
                    }
                } else if expected_revision.is_some() {
                    return SaveConversationOutcome::Conflict;
                } else {
                    let layout = Some(crate::cm_turn_layout::layout_meta_from_messages(&messages));
                    guard.insert(
                        conversation_id,
                        MemoryConversationEntry {
                            messages,
                            active_agent_role: role_owned,
                            active_session_mode: mode_owned,
                            active_permission_level: level_owned,
                            layout,
                            revision: 1,
                            updated_at: now,
                        },
                    );
                }
                Self::prune_memory_locked(&mut guard, now, r.ttl_secs, r.max_entries);
                SaveConversationOutcome::Saved
            }
            Self::Sqlite(conn) => {
                let id = conversation_id;
                let id_log = id.clone();
                let c = Arc::clone(conn);
                let exp = expected_revision;
                let active_for_sql = active_agent_role.map(|s| s.to_string());
                let mode_for_sql = active_session_mode.map(|s| s.to_string());
                let level_for_sql = active_permission_level.map(|s| s.to_string());
                sqlite_conversation_store_op(c, id_log, "保存", move |g| {
                    let outcome = conversation_store::save_with_archive_if_revision(
                        g,
                        &id,
                        messages,
                        active_for_sql.as_deref(),
                        mode_for_sql.as_deref(),
                        level_for_sql.as_deref(),
                        exp,
                        &archive_rows,
                        r.archive_codec_min_bytes,
                    )?;
                    if outcome == SaveConversationOutcome::Saved {
                        conversation_store::prune(
                            g,
                            r.ttl_secs,
                            r.max_entries,
                            r.archive_max_rows_per_conversation,
                        )?;
                    }
                    Ok(outcome)
                })
                .await
                .unwrap_or(SaveConversationOutcome::Conflict)
            }
        }
    }

    /// 截断到第 `user_ordinal` 条**普通**用户消息之前（0-based，不含长期记忆/变更集/首轮工作区画像等注入），且仅当 `revision` 匹配时成功。
    ///
    /// 内存后端无归档，不做 re-expand，故只可能返回 [`BranchTruncateOutcome::Saved`] / [`BranchTruncateOutcome::Conflict`]；
    /// SQLite 后端可能因归档前缀被回收而返回 [`BranchTruncateOutcome::ArchiveIncomplete`]（见 §3.11 / §5.3.6）。
    pub(crate) async fn truncate_before_user_ordinal_if_revision(
        &self,
        conversation_id: String,
        user_ordinal: usize,
        expected_revision: u64,
        r: ConversationRetention,
    ) -> BranchTruncateOutcome {
        match self {
            Self::Memory(map) => {
                let mut guard = map.write().await;
                let Some(entry) = guard.get_mut(&conversation_id) else {
                    return BranchTruncateOutcome::Conflict;
                };
                if r.ttl_secs > 0 && entry.updated_at.elapsed() > Duration::from_secs(r.ttl_secs) {
                    guard.remove(&conversation_id);
                    return BranchTruncateOutcome::Conflict;
                }
                if entry.revision != expected_revision {
                    return BranchTruncateOutcome::Conflict;
                }
                let cut = conversation_store::branch_cut_index(&entry.messages, user_ordinal);
                if cut >= entry.messages.len() {
                    entry.updated_at = std::time::Instant::now();
                    return BranchTruncateOutcome::Saved;
                }
                entry.messages.truncate(cut);
                entry.layout = Some(crate::cm_turn_layout::layout_meta_from_messages(
                    &entry.messages,
                ));
                entry.revision = entry.revision.saturating_add(1);
                entry.updated_at = std::time::Instant::now();
                Self::prune_memory_locked(
                    &mut guard,
                    std::time::Instant::now(),
                    r.ttl_secs,
                    r.max_entries,
                );
                BranchTruncateOutcome::Saved
            }
            Self::Sqlite(conn) => {
                let id = conversation_id;
                let id_log = id.clone();
                let c = Arc::clone(conn);
                sqlite_conversation_store_op(c, id_log, "截断", move |g| {
                    let outcome = conversation_store::truncate_before_user_ordinal_if_revision(
                        g,
                        &id,
                        user_ordinal,
                        expected_revision,
                    )?;
                    if outcome == BranchTruncateOutcome::Saved {
                        conversation_store::prune(
                            g,
                            r.ttl_secs,
                            r.max_entries,
                            r.archive_max_rows_per_conversation,
                        )?;
                    }
                    Ok(outcome)
                })
                .await
                .unwrap_or(BranchTruncateOutcome::Conflict)
            }
        }
    }

    /// 只改会话级 `active_*` 意图列（`POST /conversation/state`）。
    ///
    /// `Ok(false)` = 会话不存在；`Err` = 存储层失败（锁中毒 / SQLite 报错 / 任务崩）。
    /// 两者必须分开：把存储失败压成 `false` 会让「磁盘满」对客户端表现为「会话不存在」。
    pub(crate) async fn set_active_state(
        &self,
        conversation_id: String,
        role: Option<&str>,
        mode: Option<&str>,
        level: Option<&str>,
    ) -> Result<bool, String> {
        match self {
            Self::Memory(map) => {
                let mut guard = map.write().await;
                match guard.get_mut(&conversation_id) {
                    Some(entry) => {
                        entry.apply_active_state(role, mode, level);
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
            Self::Sqlite(conn) => {
                let c = Arc::clone(conn);
                // 闭包要求 `'static`：借用无法跨 `spawn_blocking`，先把三列取值复制成 owned。
                let role = role.map(str::to_string);
                let mode = mode.map(str::to_string);
                let level = level.map(str::to_string);
                sqlite_conversation_store_op(c, conversation_id.clone(), "写入会话状态", move |g| {
                    crate::conversation_store::set_active_state(
                        g,
                        &conversation_id,
                        role.as_deref(),
                        mode.as_deref(),
                        level.as_deref(),
                    )
                })
                .await
            }
        }
    }

    /// 会话正文里仍引用的 **`/uploads/<filename>`**（清理附图时跳过）。
    pub(crate) async fn referenced_upload_filenames(&self) -> HashSet<String> {
        use crate::web::chat_uploads_paths::collect_upload_filenames_from_text;
        match self {
            Self::Memory(map) => {
                let guard = map.read().await;
                let mut out = HashSet::new();
                for entry in guard.values() {
                    if let Ok(s) = serde_json::to_string(&entry.messages) {
                        out.extend(collect_upload_filenames_from_text(&s));
                    }
                }
                out
            }
            Self::Sqlite(conn) => {
                let c = Arc::clone(conn);
                tokio::task::spawn_blocking(move || {
                    let g = match c.lock() {
                        Ok(g) => g,
                        Err(_) => return HashSet::new(),
                    };
                    let Ok(blobs) = conversation_store::list_all_messages_json(&g) else {
                        return HashSet::new();
                    };
                    let mut out = HashSet::new();
                    for s in blobs {
                        out.extend(collect_upload_filenames_from_text(&s));
                    }
                    out
                })
                .await
                .unwrap_or_default()
            }
        }
    }

    pub(crate) async fn conversation_count(&self) -> usize {
        match self {
            Self::Memory(map) => map.read().await.len(),
            Self::Sqlite(conn) => {
                let c = Arc::clone(conn);
                tokio::task::spawn_blocking(move || {
                    let g = match c.lock() {
                        Ok(g) => g,
                        Err(_) => return 0usize,
                    };
                    conversation_store::count(&g).unwrap_or(0)
                })
                .await
                .unwrap_or(0)
            }
        }
    }

    /// 删除持久化会话行；**幂等**（不存在时静默成功）。
    ///
    /// 供 Web `DELETE /conversation/{conversation_id}` 与 E2E 夹具 `replace` 使用。
    pub(crate) async fn delete(&self, conversation_id: &str) {
        match self {
            Self::Memory(map) => {
                let mut guard = map.write().await;
                guard.remove(conversation_id);
            }
            Self::Sqlite(conn) => {
                let id = conversation_id.to_string();
                let c = Arc::clone(conn);
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(g) = c.lock() {
                        let _ = conversation_store::delete_by_id(&g, &id);
                    }
                })
                .await;
            }
        }
    }

    fn prune_memory_locked(
        guard: &mut HashMap<String, MemoryConversationEntry>,
        now: std::time::Instant,
        ttl_secs: u64,
        max_entries: usize,
    ) {
        if ttl_secs > 0 {
            let ttl = Duration::from_secs(ttl_secs);
            guard.retain(|_, v| now.duration_since(v.updated_at) <= ttl);
        }
        if max_entries == 0 || guard.len() <= max_entries {
            return;
        }
        let mut order: Vec<(String, std::time::Instant)> = guard
            .iter()
            .map(|(k, v)| (k.clone(), v.updated_at))
            .collect();
        order.sort_by_key(|(_, t)| *t);
        let to_drop = guard.len() - max_entries;
        for (k, _) in order.into_iter().take(to_drop) {
            guard.remove(&k);
        }
    }
}

/// 打开 SQLite 会话库（`run()` 在 `serve` 时调用）。
pub(crate) fn open_conversation_sqlite(
    path: &Path,
) -> Result<Arc<std::sync::Mutex<rusqlite::Connection>>, Box<dyn std::error::Error + Send + Sync>> {
    let conn = conversation_store::open_file(path)?;
    if let Err(e) = LongTermMemoryRuntime::migrate_on_connection(&conn) {
        return Err(format!("长期记忆表迁移失败: {e}").into());
    }
    Ok(Arc::new(std::sync::Mutex::new(conn)))
}
