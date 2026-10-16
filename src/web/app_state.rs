//! Web 服务进程内状态：会话存储、上传目录、任务队列句柄等（自 `lib.rs` 下沉）。

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use tokio::sync::mpsc;

use crate::chat_job_queue::{ChatJobQueue, WebChatQueueDeps};
use crate::config::SharedAgentConfig;
use crate::cm_api_contract::chat::ConversationLayoutMeta;
use crate::conversation_store::{self, BranchTruncateOutcome, SaveConversationOutcome};
use crate::memory::long_term_memory::LongTermMemoryRuntime;
use crate::types::{CommandApprovalDecision, Message};

use crate::sse::SseStreamHub;

/// 与 `chat_handlers::normalize_client_conversation_id` 及存储上限对齐。
pub(crate) use crate::conversation_store::CONVERSATION_ID_MAX_LEN;

/// Web **`POST /chat/stream`** 携带 `approval_session_id` 时注册的 **`POST /chat/approval`** 投递通道；带创建时刻以便惰性淘汰陈旧条目。
pub(crate) struct ApprovalSessionSlot {
    pub(crate) tx: mpsc::Sender<CommandApprovalDecision>,
    pub(crate) created_at: std::time::Instant,
}

/// 未完成审批决策的会话在内存中的保留时长（与 worker 结束时 `remove` 互补）。
pub(crate) const APPROVAL_SESSION_TTL: Duration = Duration::from_secs(3600);

pub(crate) fn purge_expired_approval_sessions(
    map: &mut HashMap<String, ApprovalSessionSlot>,
    ttl: Duration,
) {
    let now = std::time::Instant::now();
    map.retain(|_, slot| now.duration_since(slot.created_at) <= ttl);
}

#[derive(Clone)]
pub(crate) struct MemoryConversationEntry {
    messages: Vec<Message>,
    /// 当前多角色工作台选用的命名角色 id；`None` 表示默认人格（与 Web 未持久化选用一致）。
    active_agent_role: Option<String>,
    /// 当前会话工作模式；`None` 表示未显式设置（回落配置默认）。
    active_session_mode: Option<String>,
    /// B2：与 SQLite `layout_meta_json` 同语义，每次按当前 `messages` 派生写入。
    layout: Option<ConversationLayoutMeta>,
    revision: u64,
    updated_at: std::time::Instant,
}

#[derive(Clone)]
pub(crate) struct ConversationTurnSeed {
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    pub persisted_active_agent_role: Option<String>,
    /// 持久化的会话工作模式（ask/plan/act）；`None` 表示未设置。
    pub persisted_active_session_mode: Option<String>,
    /// 可选布局元数据；缺省时 GET 省略。会话级，不随消息分页切片。
    pub layout: Option<ConversationLayoutMeta>,
}

fn nonempty_persisted_column(raw: String) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// HTTP 客户端、共享配置快照与工作区覆盖（与队列 / 会话后端解耦）。
///
/// 根包保留本类型以满足 axum `FromRef` 孤儿规则（**不**在 `cm_web_host` 镜像同名类型）。
#[derive(Clone)]
pub(crate) struct AppStateHttpCore {
    pub(crate) cfg: SharedAgentConfig,
    /// 与启动时 `--config` / 默认探测一致，供 **`POST /config/reload`** 调用 [`load_config`]。
    pub(crate) config_path_for_reload: Option<String>,
    /// 进程级 Bearer 密钥快照（`Arc` 便于 handler facet 廉价共享，勿整串 `Clone`）。
    pub(crate) api_key: Arc<str>,
    pub(crate) client: reqwest::Client,
    pub(crate) tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.command_exec.run_command_working_dir
    pub(crate) workspace_override: Arc<tokio::sync::RwLock<Option<String>>>,
    pub(crate) uploads_dir: std::path::PathBuf,
}

/// 由工作区覆盖 + 共享配置解析当前有效工作区根（空串表示未设置）。
pub(crate) async fn effective_workspace_path_from_override(
    workspace_override: &tokio::sync::RwLock<Option<String>>,
    cfg: &SharedAgentConfig,
) -> String {
    let guard = workspace_override.read().await;
    match guard.as_deref() {
        None => String::new(),
        Some(s) if s.trim().is_empty() => {
            let cfg = cfg.read().await;
            cfg.command_exec.run_command_working_dir.clone()
        }
        Some(s) => s.to_string(),
    }
}

/// 前端是否已经设置过明确工作区路径（`Some(non-empty)`）。
pub(crate) async fn workspace_is_set_from_override(
    workspace_override: &tokio::sync::RwLock<Option<String>>,
) -> bool {
    let guard = workspace_override.read().await;
    guard.as_deref().is_some_and(|s| !s.trim().is_empty())
}

impl AppStateHttpCore {
    /// 当前 Web 会话选中的工作区根路径（**未**调用 `POST /workspace` 成功设置前返回空串）。
    pub(crate) async fn effective_workspace_path(&self) -> String {
        effective_workspace_path_from_override(&self.workspace_override, &self.cfg).await
    }
}

/// `/chat` / `/chat/stream` 进程内队列及其 worker 依赖。
#[derive(Clone)]
pub(crate) struct AppStateChatRuntime {
    pub(crate) chat_queue: ChatJobQueue,
    pub(crate) chat_queue_job_deps: Arc<WebChatQueueDeps>,
}

/// 会话持久化与默认 `conversation_id` 生成。
#[derive(Clone)]
pub(crate) struct AppStateConversationRuntime {
    /// `conversation_id` → 消息与 revision：内存或 SQLite（见配置 `conversation_store_sqlite_path`）。
    /// 外层 `RwLock` 供 Web **`POST /config/session/conversation-store`** 在进程内切换后端（与配置热重载不同轨）。
    pub(crate) conversation_backing: Arc<tokio::sync::RwLock<ConversationBacking>>,
    pub(crate) conversation_id_counter: Arc<AtomicU64>,
    /// 共享配置快照：保留策略（TTL / 条数上限）在每次落盘与后台 prune 时读取，支持热更。
    pub(crate) cfg: SharedAgentConfig,
}

/// `GET /status` tiktoken 新会话基线的缓存条目。
///
/// 命中条件：`cfg_fingerprint`（全量 `AgentConfig` Debug 哈希）与 `workspace_root` 一致
/// 且未超过 TTL（见 `health_status::BASELINE_CACHE_TTL`）；工作区文件内容变化不主动失效。
#[derive(Clone)]
pub(crate) struct CachedTiktokenBaseline {
    pub(crate) cfg_fingerprint: u64,
    pub(crate) workspace_root: String,
    pub(crate) computed_at: std::time::Instant,
    pub(crate) baseline_by_role: std::collections::BTreeMap<String, u32>,
}

/// 审批表、任务侧栏、SSE hub、异步作业等 Web 辅助状态。
#[derive(Clone)]
pub(crate) struct AppStateWebAux {
    pub(crate) approval_sessions: Arc<tokio::sync::RwLock<HashMap<String, ApprovalSessionSlot>>>,
    pub(crate) long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    pub(crate) llm_models_health_cache:
        Arc<std::sync::Mutex<Option<crate::health::CachedLlmModelsHealthProbe>>>,
    /// `GET /status` tiktoken 新会话基线缓存（大工作区同步分词可达数十秒，见 `health_status`）。
    pub(crate) tiktoken_baseline_cache:
        Arc<std::sync::Mutex<Option<CachedTiktokenBaseline>>>,
    pub(crate) sse_stream_hub: Arc<SseStreamHub>,
    pub(crate) process_handles: Arc<crate::process_handles::ProcessHandles>,
    pub(crate) async_chat_jobs: super::async_chat_job::AsyncChatJobsMap,
    /// 后台任务注册表（`run_command` 的 `async=true`）；启动时按 `[tool_registry]` 配置构建。
    pub(crate) tool_job_registry: std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) http: AppStateHttpCore,
    pub(crate) chat: AppStateChatRuntime,
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) aux: AppStateWebAux,
}

/// 队列 worker 所需的 AppState **消费面**：会话落盘、审批表、`ProcessHandles`（不含 HTTP/上传/整包状态）。
///
/// 与 [`crate::chat_job_queue::WebChatQueueDeps`] 互补：后者管 LLM/工具/SSE hub；本类型管回合后落盘与审批清理。
#[derive(Clone)]
pub(crate) struct WebChatJobAppFacet {
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) process_handles: Arc<crate::process_handles::TurnProcessHandles>,
    pub(crate) approval_sessions: Arc<tokio::sync::RwLock<HashMap<String, ApprovalSessionSlot>>>,
    /// 后台任务注册表（`run_command` 的 `async=true`）。
    pub(crate) tool_job_registry: std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
}

/// Web 会话存储后端。
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
}

async fn sqlite_conversation_store_op<T>(
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
    id_log: String,
    op_zh: &'static str,
    fallback: T,
    run: impl FnOnce(&rusqlite::Connection) -> Result<T, rusqlite::Error> + Send + 'static,
) -> T
where
    T: Copy + Send + 'static,
{
    match tokio::task::spawn_blocking(move || {
        let g = conn
            .lock()
            .map_err(|e: std::sync::PoisonError<_>| e.to_string())?;
        run(&g).map_err(|e: rusqlite::Error| e.to_string())
    })
    .await
    {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => {
            log::error!(
                target: "crabmate",
                "会话 SQLite {}失败 conversation_id={} error={}",
                op_zh,
                id_log,
                e
            );
            fallback
        }
        Err(e) => {
            log::error!(
                target: "crabmate",
                "会话 SQLite {}任务失败 conversation_id={} error={}",
                op_zh,
                id_log,
                e
            );
            fallback
        }
    }
}

/// 会话保留与归档回收策略快照（一次读锁内取齐，见设计文档 §3.8 / §4.2）。
struct ConversationRetention {
    /// `0` = 不过期。
    ttl_secs: u64,
    /// `0` = 不限条数。
    max_entries: usize,
    /// 单会话归档行数上限；`0` = 不限（§4.2 / C4）。
    archive_max_rows_per_conversation: usize,
    /// 归档 codec 选择阈值（字节，§3.8）。
    archive_codec_min_bytes: usize,
}

impl AppStateConversationRuntime {
    /// 当前配置的会话保留 / 归档策略：`ttl_secs == 0` 不过期；`max_entries == 0` 不限条数；
    /// `archive_max_rows_per_conversation == 0` 不限归档行数。
    async fn conversation_retention(&self) -> ConversationRetention {
        let cfg = self.cfg.read().await;
        let p = &cfg.conversation_persistence;
        ConversationRetention {
            ttl_secs: p.conversation_store_ttl_secs,
            max_entries: p.conversation_store_max_entries,
            archive_max_rows_per_conversation: p.context_archive_max_rows_per_conversation,
            archive_codec_min_bytes: p.context_archive_codec_min_bytes,
        }
    }

    /// 按当前配置清理过期 / 超量会话（`serve` 启动时与低频定时任务调用）。
    pub(crate) async fn prune_conversations_now(&self) {
        let r = self.conversation_retention().await;
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                Self::prune_memory_locked(
                    &mut guard,
                    std::time::Instant::now(),
                    r.ttl_secs,
                    r.max_entries,
                );
            }
            ConversationBacking::Sqlite(conn) => {
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

    pub(crate) async fn load_conversation_seed(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationTurnSeed> {
        let ttl_secs = self.conversation_retention().await.ttl_secs;
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
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
                    layout: entry.layout.clone(),
                })
            }
            ConversationBacking::Sqlite(conn) => {
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
                    layout: row.layout,
                })
            }
        }
    }

    /// 读取会话归档原文（`GET /conversation/messages` 带 `include_archive=true`，见设计文档 §5.3.4 C2）。
    ///
    /// 仅 SQLite 后端持久化归档；内存后端无归档，返回空。锁 / 读取 / 解码失败记录日志并返回空，
    /// 不阻断快照响应（与 `list_all_messages_json` 对归档失败的容错口径一致）。
    pub(crate) async fn load_conversation_archive_messages(
        &self,
        conversation_id: &str,
    ) -> Vec<Message> {
        let backing = self.conversation_backing.read().await;
        let ConversationBacking::Sqlite(conn) = &*backing else {
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

    pub(crate) async fn save_conversation_messages_if_revision(
        &self,
        conversation_id: String,
        messages: Vec<Message>,
        active_agent_role: Option<&str>,
        active_session_mode: Option<&str>,
        expected_revision: Option<u64>,
        archive_rows: Vec<crate::conversation_store::ArchiveRow>,
    ) -> SaveConversationOutcome {
        let r = self.conversation_retention().await;
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                let now = std::time::Instant::now();
                let role_owned = active_agent_role
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let mode_owned = active_session_mode
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                if let Some(entry) = guard.get_mut(&conversation_id) {
                    match expected_revision {
                        Some(exp) if entry.revision == exp => {
                            entry.layout = Some(crate::cm_turn_layout::layout_meta_from_messages(
                                &messages,
                            ));
                            entry.messages = messages;
                            entry.active_agent_role = role_owned;
                            entry.active_session_mode = mode_owned;
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
                            layout,
                            revision: 1,
                            updated_at: now,
                        },
                    );
                }
                Self::prune_memory_locked(&mut guard, now, r.ttl_secs, r.max_entries);
                SaveConversationOutcome::Saved
            }
            ConversationBacking::Sqlite(conn) => {
                let id = conversation_id;
                let id_log = id.clone();
                let c = Arc::clone(conn);
                let exp = expected_revision;
                let active_for_sql = active_agent_role.map(|s| s.to_string());
                let mode_for_sql = active_session_mode.map(|s| s.to_string());
                sqlite_conversation_store_op(
                    c,
                    id_log,
                    "保存",
                    SaveConversationOutcome::Conflict,
                    move |g| {
                        let outcome = conversation_store::save_with_archive_if_revision(
                            g,
                            &id,
                            messages,
                            active_for_sql.as_deref(),
                            mode_for_sql.as_deref(),
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
                    },
                )
                .await
            }
        }
    }

    /// 截断到第 `user_ordinal` 条**普通**用户消息之前（0-based，不含长期记忆/变更集/首轮工作区画像等注入），且仅当 `revision` 匹配时成功。
    ///
    /// 内存后端无归档，不做 re-expand，故只可能返回 [`BranchTruncateOutcome::Saved`] / [`BranchTruncateOutcome::Conflict`]；
    /// SQLite 后端可能因归档前缀被回收而返回 [`BranchTruncateOutcome::ArchiveIncomplete`]（见 §3.11 / §5.3.6）。
    pub(crate) async fn truncate_conversation_before_user_ordinal_if_revision(
        &self,
        conversation_id: String,
        user_ordinal: usize,
        expected_revision: u64,
    ) -> BranchTruncateOutcome {
        let r = self.conversation_retention().await;
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
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
            ConversationBacking::Sqlite(conn) => {
                let id = conversation_id;
                let id_log = id.clone();
                let c = Arc::clone(conn);
                sqlite_conversation_store_op(
                    c,
                    id_log,
                    "截断",
                    BranchTruncateOutcome::Conflict,
                    move |g| {
                        let outcome =
                            conversation_store::truncate_before_user_ordinal_if_revision(
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
                    },
                )
                .await
            }
        }
    }

    /// 会话正文里仍引用的 **`/uploads/<filename>`**（清理附图时跳过）。
    pub(crate) async fn referenced_upload_filenames(&self) -> HashSet<String> {
        use crate::web::chat_uploads_paths::collect_upload_filenames_from_text;
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
                let guard = map.read().await;
                let mut out = HashSet::new();
                for entry in guard.values() {
                    if let Ok(s) = serde_json::to_string(&entry.messages) {
                        out.extend(collect_upload_filenames_from_text(&s));
                    }
                }
                out
            }
            ConversationBacking::Sqlite(conn) => {
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
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => map.read().await.len(),
            ConversationBacking::Sqlite(conn) => {
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
    pub(crate) async fn delete_conversation_record(&self, conversation_id: &str) {
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                guard.remove(conversation_id);
            }
            ConversationBacking::Sqlite(conn) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> SharedAgentConfig {
        Arc::new(tokio::sync::RwLock::new(
            crate::config::load_config(None).expect("默认配置加载失败"),
        ))
    }

    async fn runtime(sqlite: bool) -> AppStateConversationRuntime {
        let backing = if sqlite {
            let conn = rusqlite::Connection::open_in_memory().expect("sqlite");
            conversation_store::migrate(&conn).expect("migrate");
            ConversationBacking::Sqlite(Arc::new(std::sync::Mutex::new(conn)))
        } else {
            ConversationBacking::memory_default()
        };
        AppStateConversationRuntime {
            conversation_backing: Arc::new(tokio::sync::RwLock::new(backing)),
            conversation_id_counter: Arc::new(AtomicU64::new(1)),
            cfg: test_cfg(),
        }
    }

    async fn set_retention(rt: &AppStateConversationRuntime, ttl_secs: u64, max_entries: usize) {
        let mut g = rt.cfg.write().await;
        g.conversation_persistence.conversation_store_ttl_secs = ttl_secs;
        g.conversation_persistence.conversation_store_max_entries = max_entries;
    }

    fn one_user_msg(text: &str) -> Vec<Message> {
        vec![Message::user_only(text.to_string())]
    }

    async fn save(rt: &AppStateConversationRuntime, id: &str) {
        assert_eq!(
            rt.save_conversation_messages_if_revision(
                id.to_string(),
                one_user_msg("hi"),
                None,
                None,
                None,
                Vec::new(),
            )
            .await,
            SaveConversationOutcome::Saved
        );
    }

    #[tokio::test]
    async fn delete_conversation_record_removes_row_and_is_idempotent() {
        for sqlite in [false, true] {
            let rt = runtime(sqlite).await;
            save(&rt, "c1").await;
            assert!(rt.load_conversation_seed("c1").await.is_some());
            rt.delete_conversation_record("c1").await;
            assert!(
                rt.load_conversation_seed("c1").await.is_none(),
                "sqlite={sqlite}"
            );
            // 幂等：重复删除不 panic
            rt.delete_conversation_record("c1").await;
            rt.delete_conversation_record("never-existed").await;
        }
    }

    #[tokio::test]
    async fn retention_zero_disables_pruning_and_hot_reload_applies_on_next_save() {
        for sqlite in [false, true] {
            let rt = runtime(sqlite).await;
            // `0` = 不过期 / 不限条数。
            set_retention(&rt, 0, 0).await;
            for id in ["c1", "c2", "c3"] {
                save(&rt, id).await;
            }
            assert_eq!(rt.conversation_count().await, 3, "sqlite={sqlite}");

            // 热更条数上限为 1：下一次 save 触发 prune。
            set_retention(&rt, 0, 1).await;
            save(&rt, "c4").await;
            assert_eq!(rt.conversation_count().await, 1, "sqlite={sqlite}");
        }
    }

    #[tokio::test]
    async fn prune_conversations_now_keeps_rows_when_ttl_zero() {
        for sqlite in [false, true] {
            let rt = runtime(sqlite).await;
            set_retention(&rt, 0, 0).await;
            save(&rt, "c1").await;
            save(&rt, "c2").await;
            rt.prune_conversations_now().await;
            assert_eq!(rt.conversation_count().await, 2, "sqlite={sqlite}");
        }
    }

    #[tokio::test]
    async fn load_conversation_archive_messages_only_for_sqlite() {
        // 内存后端不持久化归档：恒为空（C2 语义）。
        let mem = runtime(false).await;
        save(&mem, "c1").await;
        assert!(mem.load_conversation_archive_messages("c1").await.is_empty());

        // SQLite 后端：带归档行保存后可读回（按 seq 升序）。
        let sq = runtime(true).await;
        let archive_rows = vec![crate::conversation_store::ArchiveRow {
            reason: crate::conversation_store::ArchiveReason::RemovedTurnGroup,
            range_start: 0,
            range_end: 1,
            messages: one_user_msg("archived"),
        }];
        assert_eq!(
            sq.save_conversation_messages_if_revision(
                "c1".to_string(),
                one_user_msg("hi"),
                None,
                None,
                None,
                archive_rows,
            )
            .await,
            SaveConversationOutcome::Saved
        );
        let archive = sq.load_conversation_archive_messages("c1").await;
        assert_eq!(archive.len(), 1);
        assert_eq!(archive[0].role, "user");
        // 未归档 / 不存在的会话返回空。
        assert!(sq.load_conversation_archive_messages("nope").await.is_empty());
    }

    /// §9.2 item6：被归档消息引用的 `/uploads/<filename>` 不得被清理逻辑判为孤儿。
    #[tokio::test]
    async fn referenced_upload_filenames_include_archived_messages() {
        // 内存后端不持久化归档（C2 语义）：归档引用无法被扫描到。
        let mem = runtime(false).await;
        assert_eq!(
            mem.save_conversation_messages_if_revision(
                "c1".to_string(),
                one_user_msg("canonical 无引用"),
                None,
                None,
                None,
                vec![crate::conversation_store::ArchiveRow {
                    reason: crate::conversation_store::ArchiveReason::Summarized,
                    range_start: 0,
                    range_end: 1,
                    messages: one_user_msg("附图 /uploads/archived_pic.png 见上"),
                }],
            )
            .await,
            SaveConversationOutcome::Saved
        );
        assert!(
            mem.referenced_upload_filenames().await.is_empty(),
            "内存后端不落盘归档，扫描不到被归档引用"
        );

        // SQLite 后端：归档行随 canonical 一同落盘，引用必须被扫描到。
        let sq = runtime(true).await;
        assert_eq!(
            sq.save_conversation_messages_if_revision(
                "c1".to_string(),
                one_user_msg("canonical 正文无附件引用"),
                None,
                None,
                None,
                vec![crate::conversation_store::ArchiveRow {
                    reason: crate::conversation_store::ArchiveReason::Summarized,
                    range_start: 0,
                    range_end: 1,
                    messages: one_user_msg("附图 /uploads/archived_pic.png 见上"),
                }],
            )
            .await,
            SaveConversationOutcome::Saved
        );
        let refs = sq.referenced_upload_filenames().await;
        assert!(
            refs.contains("archived_pic.png"),
            "被归档消息引用的 /uploads 文件不得判为孤儿: {refs:?}"
        );
    }
}
