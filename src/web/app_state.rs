//! Web 服务进程内状态：会话存储、上传目录、任务队列句柄等（自 `lib.rs` 下沉）。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;

use crate::chat_job_queue::{ChatJobQueue, WebChatQueueDeps};
use crate::config::SharedAgentConfig;
use crate::conversation_store::{
    self, CONVERSATION_STORE_MAX_ENTRIES, CONVERSATION_STORE_TTL_SECS, SaveConversationOutcome,
};
use crate::long_term_memory::LongTermMemoryRuntime;
use crate::types::{CommandApprovalDecision, Message};

use super::http_types::tasks::TasksData;
use crate::sse::SseStreamHub;

/// 与 `normalize_client_conversation_id`（`chat_handlers`）及存储上限对齐。
pub(crate) const CONVERSATION_ID_MAX_LEN: usize = 128;
const CONVERSATION_STORE_TTL: Duration = Duration::from_secs(CONVERSATION_STORE_TTL_SECS);

#[derive(Clone)]
pub(crate) struct MemoryConversationEntry {
    messages: Vec<Message>,
    /// 当前多角色工作台选用的命名角色 id；`None` 表示默认人格（与 Web 未持久化选用一致）。
    active_agent_role: Option<String>,
    revision: u64,
    updated_at: std::time::Instant,
}

#[derive(Clone)]
pub(crate) struct ConversationTurnSeed {
    pub messages: Vec<Message>,
    pub expected_revision: Option<u64>,
    pub persisted_active_agent_role: Option<String>,
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) cfg: SharedAgentConfig,
    /// 与启动时 `--config` / 默认探测一致，供 **`POST /config/reload`** 调用 [`load_config`]。
    pub(crate) config_path_for_reload: Option<String>,
    pub(crate) api_key: String,
    pub(crate) client: reqwest::Client,
    pub(crate) tools: Vec<crate::types::Tool>,
    /// 前端设置的工作区路径覆盖；为 None 时使用 cfg.run_command_working_dir
    pub(crate) workspace_override: Arc<tokio::sync::RwLock<Option<String>>>,
    pub(crate) uploads_dir: std::path::PathBuf,
    /// `/chat` / `/chat/stream` 进程内任务队列（有界排队 + 并发上限）
    pub(crate) chat_queue: ChatJobQueue,
    /// 队列 worker 使用的 LLM/工具/hub 句柄（与会话存储等字段分离，见 [`WebChatQueueDeps`]）。
    pub(crate) chat_queue_job_deps: Arc<WebChatQueueDeps>,
    /// `conversation_id` → 消息与 revision：内存或 SQLite（见配置 `conversation_store_sqlite_path`）
    pub(crate) conversation_backing: ConversationBacking,
    /// 新会话 ID 递增计数器（仅用于生成默认 conversation_id）。
    pub(crate) conversation_id_counter: Arc<AtomicU64>,
    /// Web 流式审批会话 -> 决策通道。
    pub(crate) approval_sessions:
        Arc<tokio::sync::RwLock<HashMap<String, mpsc::Sender<CommandApprovalDecision>>>>,
    /// 长期记忆（可选 SQLite + 可选 fastembed）；未启用或未配置路径时为 `None`。
    pub(crate) long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    /// Web 侧栏任务清单：按**当前生效工作区路径**键入，仅存本进程内存（**不**写 `tasks.json`）。
    pub(crate) web_tasks_by_workspace: Arc<tokio::sync::RwLock<HashMap<String, TasksData>>>,
    /// [`GET /health`](crate::web::chat_handlers::health_handler) 可选 **GET …/models** 探测结果缓存（见 `health_llm_models_probe_cache_secs`）。
    pub(crate) llm_models_health_cache:
        Arc<std::sync::Mutex<Option<crate::health::CachedLlmModelsHealthProbe>>>,
    /// `/chat/stream` 断线重连：`Last-Event-ID` / `stream_resume` 与环形缓冲（进程内）。
    pub(crate) sse_stream_hub: Arc<SseStreamHub>,
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
}

async fn sqlite_conversation_store_op(
    conn: Arc<std::sync::Mutex<rusqlite::Connection>>,
    id_log: String,
    op_zh: &'static str,
    run: impl FnOnce(&rusqlite::Connection) -> Result<SaveConversationOutcome, rusqlite::Error>
    + Send
    + 'static,
) -> SaveConversationOutcome {
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
            SaveConversationOutcome::Conflict
        }
        Err(e) => {
            log::error!(
                target: "crabmate",
                "会话 SQLite {}任务失败 conversation_id={} error={}",
                op_zh,
                id_log,
                e
            );
            SaveConversationOutcome::Conflict
        }
    }
}

impl AppState {
    /// 当前 Web 会话选中的工作区根路径（**未**调用 `POST /workspace` 成功设置前返回空串）。
    ///
    /// 与配置项 **`run_command_working_dir`** 分离：后者仍供 CLI、配置解析、`GET /health` 等使用；Web 侧栏在首次设置前不应默认等同于进程当前目录。
    pub(crate) async fn effective_workspace_path(&self) -> String {
        let guard = self.workspace_override.read().await;
        match guard.as_deref() {
            None => String::new(),
            Some(s) if s.trim().is_empty() => {
                let cfg = self.cfg.read().await;
                cfg.run_command_working_dir.clone()
            }
            Some(s) => s.to_string(),
        }
    }

    /// 前端是否已经“设置过工作区”（包含：显式选择默认目录）
    pub(crate) async fn workspace_is_set(&self) -> bool {
        let guard = self.workspace_override.read().await;
        guard.is_some()
    }

    pub(crate) fn next_conversation_id(&self) -> String {
        let n = self.conversation_id_counter.fetch_add(1, Ordering::Relaxed);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        format!("conv_{}_{}", ts, n)
    }

    pub(crate) async fn load_conversation_seed(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationTurnSeed> {
        match &self.conversation_backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                let entry = guard.get_mut(conversation_id)?;
                if entry.updated_at.elapsed() > CONVERSATION_STORE_TTL {
                    guard.remove(conversation_id);
                    return None;
                }
                entry.updated_at = std::time::Instant::now();
                Some(ConversationTurnSeed {
                    messages: entry.messages.clone(),
                    expected_revision: Some(entry.revision),
                    persisted_active_agent_role: entry.active_agent_role.clone(),
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
                    match conversation_store::load(&g, &id, CONVERSATION_STORE_TTL_SECS) {
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
                loaded.map(|(messages, revision, active)| ConversationTurnSeed {
                    messages,
                    expected_revision: Some(revision),
                    persisted_active_agent_role: {
                        let t = active.trim();
                        if t.is_empty() {
                            None
                        } else {
                            Some(t.to_string())
                        }
                    },
                })
            }
        }
    }

    fn prune_memory_locked(
        guard: &mut HashMap<String, MemoryConversationEntry>,
        now: std::time::Instant,
    ) {
        guard.retain(|_, v| now.duration_since(v.updated_at) <= CONVERSATION_STORE_TTL);
        if guard.len() <= CONVERSATION_STORE_MAX_ENTRIES {
            return;
        }
        let mut order: Vec<(String, std::time::Instant)> = guard
            .iter()
            .map(|(k, v)| (k.clone(), v.updated_at))
            .collect();
        order.sort_by_key(|(_, t)| *t);
        let to_drop = guard.len() - CONVERSATION_STORE_MAX_ENTRIES;
        for (k, _) in order.into_iter().take(to_drop) {
            guard.remove(&k);
        }
    }

    pub(crate) async fn save_conversation_messages_if_revision(
        &self,
        conversation_id: String,
        messages: Vec<Message>,
        active_agent_role: Option<&str>,
        expected_revision: Option<u64>,
    ) -> SaveConversationOutcome {
        match &self.conversation_backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                let now = std::time::Instant::now();
                if let Some(entry) = guard.get_mut(&conversation_id) {
                    match expected_revision {
                        Some(exp) if entry.revision == exp => {
                            entry.messages = messages;
                            entry.active_agent_role = active_agent_role
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string);
                            entry.revision = entry.revision.saturating_add(1);
                            entry.updated_at = now;
                        }
                        _ => return SaveConversationOutcome::Conflict,
                    }
                } else if expected_revision.is_some() {
                    return SaveConversationOutcome::Conflict;
                } else {
                    guard.insert(
                        conversation_id,
                        MemoryConversationEntry {
                            messages,
                            active_agent_role: active_agent_role
                                .map(str::trim)
                                .filter(|s| !s.is_empty())
                                .map(str::to_string),
                            revision: 1,
                            updated_at: now,
                        },
                    );
                }
                Self::prune_memory_locked(&mut guard, now);
                SaveConversationOutcome::Saved
            }
            ConversationBacking::Sqlite(conn) => {
                let id = conversation_id;
                let id_log = id.clone();
                let c = Arc::clone(conn);
                let exp = expected_revision;
                let active_for_sql = active_agent_role.map(|s| s.to_string());
                sqlite_conversation_store_op(c, id_log, "保存", move |g| {
                    conversation_store::save_if_revision(
                        g,
                        &id,
                        messages,
                        active_for_sql.as_deref(),
                        exp,
                    )
                })
                .await
            }
        }
    }

    /// 截断到第 `user_ordinal` 条**普通**用户消息之前（0-based，不含长期记忆/变更集/首轮工作区画像等注入），且仅当 `revision` 匹配时成功。
    pub(crate) async fn truncate_conversation_before_user_ordinal_if_revision(
        &self,
        conversation_id: String,
        user_ordinal: usize,
        expected_revision: u64,
    ) -> SaveConversationOutcome {
        match &self.conversation_backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                let Some(entry) = guard.get_mut(&conversation_id) else {
                    return SaveConversationOutcome::Conflict;
                };
                if entry.updated_at.elapsed() > CONVERSATION_STORE_TTL {
                    guard.remove(&conversation_id);
                    return SaveConversationOutcome::Conflict;
                }
                if entry.revision != expected_revision {
                    return SaveConversationOutcome::Conflict;
                }
                let mut u = 0usize;
                let mut cut = entry.messages.len();
                for (i, m) in entry.messages.iter().enumerate() {
                    if crate::types::user_message_counts_for_branch_truncation(m) {
                        if u == user_ordinal {
                            cut = i;
                            break;
                        }
                        u += 1;
                    }
                }
                if cut >= entry.messages.len() {
                    entry.updated_at = std::time::Instant::now();
                    return SaveConversationOutcome::Saved;
                }
                entry.messages.truncate(cut);
                entry.revision = entry.revision.saturating_add(1);
                entry.updated_at = std::time::Instant::now();
                Self::prune_memory_locked(&mut guard, std::time::Instant::now());
                SaveConversationOutcome::Saved
            }
            ConversationBacking::Sqlite(conn) => {
                let id = conversation_id;
                let id_log = id.clone();
                let c = Arc::clone(conn);
                sqlite_conversation_store_op(c, id_log, "截断", move |g| {
                    conversation_store::truncate_before_user_ordinal_if_revision(
                        g,
                        &id,
                        user_ordinal,
                        expected_revision,
                    )
                })
                .await
            }
        }
    }

    pub(crate) async fn conversation_count(&self) -> usize {
        match &self.conversation_backing {
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
}

/// 打开 SQLite 会话库（`run()` 在 `--serve` 时调用）。
pub(crate) fn open_conversation_sqlite(
    path: &Path,
) -> Result<Arc<std::sync::Mutex<rusqlite::Connection>>, Box<dyn std::error::Error + Send + Sync>> {
    let conn = conversation_store::open_file(path)?;
    if let Err(e) = LongTermMemoryRuntime::migrate_on_connection(&conn) {
        return Err(format!("长期记忆表迁移失败: {e}").into());
    }
    Ok(Arc::new(std::sync::Mutex::new(conn)))
}
