//! Handler 侧 AppState 投影（axum `FromRef`），避免窄路由持有整包 [`AppState`]。
//!
//! 与队列侧 [`WebChatJobAppFacet`](super::app_state::WebChatJobAppFacet) 同族；见 `docs/design/web_host_extract.md`、
//! `docs/design/turn_host_decouple.md`（P3b / P3c）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use axum::extract::FromRef;

use crate::config::SharedAgentConfig;
use crate::conversation_store::{BranchTruncateOutcome, SaveConversationOutcome};
use crate::health::CachedLlmModelsHealthProbe;
use crate::process_handles::ProcessHandles;
use crate::sse::SseStreamHub;
use crate::web::async_chat_job::AsyncChatJobsMap;

use crate::chat_job_queue::ChatJobQueue;
use crate::memory::long_term_memory::LongTermMemoryRuntime;

use super::app_state::{
    AppState, AppStateChatRuntime, AppStateConversationRuntime, AppStateHttpCore,
    ApprovalSessionSlot, CachedTiktokenBaseline, ConversationBacking, ConversationTurnSeed,
    WebChatJobAppFacet, effective_workspace_path_from_override, open_conversation_sqlite,
    workspace_is_set_from_override,
};

/// 与 [`super::app_state::sqlite_conversation_store_op`] 同构，但**把失败上抛**。
///
/// 需要区分「业务上的否」与「存储层失败」时用它：`fallback` 会把两者压成同一个值，
/// 于是 `POST /conversation/state` 的库写失败（锁中毒 / SQLite 报错 / `spawn_blocking` 崩）
/// 会被 handler 当成「会话不存在」回 404，把排查方向带偏。
///
/// 放在本文件而不是 `app_state.rs`：那里已经贴着 **920 行**的文件行数棘轮上限
/// （`scripts/fn-nloc-ratchet.sh`），而这里正是它唯一的调用方。
pub(crate) async fn sqlite_conversation_store_op_result<T>(
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

impl AppStateConversationRuntime {
    /// 只改会话级 `active_*` 意图列（`POST /conversation/state`）。
    ///
    /// `Ok(false)` = 会话不存在；`Err` = 存储层失败（锁中毒 / SQLite 报错 / 任务崩）。
    /// 两者必须分开：把存储失败压成 `false` 会让「磁盘满」对客户端表现为「会话不存在」。
    ///
    /// 与 [`Self::save_conversation_messages_if_revision`]（整行覆盖 + CAS）分工：
    /// 这条**不**碰 `messages_json`、**不** bump `revision`、**不**动 `updated_at_unix`，
    /// 理由见 [`crate::conversation_store::set_active_state`]。
    pub(crate) async fn set_conversation_active_state(
        &self,
        conversation_id: String,
        role: Option<&str>,
        mode: Option<&str>,
        level: Option<&str>,
    ) -> Result<bool, String> {
        let backing = self.conversation_backing.read().await;
        match &*backing {
            ConversationBacking::Memory(map) => {
                let mut guard = map.write().await;
                match guard.get_mut(&conversation_id) {
                    Some(entry) => {
                        entry.apply_active_state(role, mode, level);
                        Ok(true)
                    }
                    None => Ok(false),
                }
            }
            ConversationBacking::Sqlite(conn) => {
                let c = Arc::clone(conn);
                // 闭包要求 `'static`：借用无法跨 `spawn_blocking`，先把三列取值复制成 owned。
                let role = role.map(str::to_string);
                let mode = mode.map(str::to_string);
                let level = level.map(str::to_string);
                sqlite_conversation_store_op_result(
                    c,
                    conversation_id.clone(),
                    "写入会话状态",
                    move |g| {
                        crate::conversation_store::set_active_state(
                            g,
                            &conversation_id,
                            role.as_deref(),
                            mode.as_deref(),
                            level.as_deref(),
                        )
                    },
                )
                .await
            }
        }
    }
}

/// `POST /config/reload`：共享配置 + 重载路径。
#[derive(Clone)]
pub(crate) struct ConfigReloadFacet {
    pub(crate) cfg: SharedAgentConfig,
    pub(crate) config_path_for_reload: Option<String>,
}

/// `POST /upload` / `POST /uploads/delete` / `GET /uploads/{filename}`：当前附图目录。
#[derive(Clone)]
pub(crate) struct UploadsFacet {
    pub(crate) cfg: SharedAgentConfig,
    pub(crate) fallback_uploads_dir: PathBuf,
}

impl UploadsFacet {
    pub(crate) async fn uploads_dir(&self) -> PathBuf {
        self.cfg
            .read()
            .await
            .chat_uploads_dir
            .clone()
            .unwrap_or_else(|| self.fallback_uploads_dir.clone())
    }
}

/// `GET|PUT /tasks` 与 `GET /workspace/changelog`：工作区键 + 完整 [`ProcessHandles`]。
#[derive(Clone)]
pub(crate) struct WebTasksAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) process_handles: Arc<ProcessHandles>,
}

/// `GET /workspace/changelog` 与 tasks 同构（共用 [`FromRef`]）。
pub(crate) type WebChangelogAppFacet = WebTasksAppFacet;

/// `GET /health`：HTTP 核 + 模型探测缓存。
#[derive(Clone)]
pub(crate) struct WebHealthAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) llm_models_health_cache: Arc<std::sync::Mutex<Option<CachedLlmModelsHealthProbe>>>,
}

/// `GET /status`：配置/工具/队列/会话/LTM/工具统计（不含审批、SSE hub、async jobs）。
#[derive(Clone)]
pub(crate) struct WebStatusAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) chat_queue: ChatJobQueue,
    pub(crate) long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    pub(crate) process_handles: Arc<ProcessHandles>,
    pub(crate) tiktoken_baseline_cache: Arc<std::sync::Mutex<Option<CachedTiktokenBaseline>>>,
}

/// `GET /memory/list` / `DELETE /memory/{id}`：共享配置 + 长期记忆运行时。
#[derive(Clone)]
pub(crate) struct MemoryAppFacet {
    pub(crate) cfg: SharedAgentConfig,
    pub(crate) long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
}

/// 窄 chat 控制面：共享配置、会话读写、审批投递。
///
/// **不含** 整份 [`AppStateHttpCore`]（避免每次 `FromRef` 克隆 `tools`）、
/// 也不含 queue / SSE hub / `async_chat_jobs`（回合入口见 [`WebChatTurnAppFacet`]）。
/// 供 `POST /chat/approval`、`POST /chat/branch`、`GET /conversation/messages`、
/// `POST /config/session/conversation-store` 等窄路由（turn_host **P3b**）。
#[derive(Clone)]
pub(crate) struct WebChatAppFacet {
    pub(crate) cfg: SharedAgentConfig,
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) approval_sessions: Arc<tokio::sync::RwLock<HashMap<String, ApprovalSessionSlot>>>,
}

/// 回合入队面：`POST /chat`、`/chat/stream`、`/chat/async`（及 job status）。
///
/// 含 queue、审批、会话、工作区覆盖、`api_key`、SSE hub、async jobs、HTTP client；
/// **仍不含** `tools` / uploads / config reload 路径（turn_host **P3c**）。
#[derive(Clone)]
pub(crate) struct WebChatTurnAppFacet {
    pub(crate) cfg: SharedAgentConfig,
    pub(crate) api_key: Arc<str>,
    pub(crate) client: reqwest::Client,
    pub(crate) workspace_override: Arc<tokio::sync::RwLock<Option<String>>>,
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) chat: AppStateChatRuntime,
    pub(crate) approval_sessions: Arc<tokio::sync::RwLock<HashMap<String, ApprovalSessionSlot>>>,
    pub(crate) process_handles: Arc<ProcessHandles>,
    pub(crate) sse_stream_hub: Arc<SseStreamHub>,
    pub(crate) async_chat_jobs: AsyncChatJobsMap,
    /// 后台任务注册表（`run_command` 的 `async=true`）。
    pub(crate) tool_job_registry: std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>,
}

/// 仅 `GET` 异步任务状态：只持 `async_chat_jobs`。
#[derive(Clone)]
pub(crate) struct AsyncChatJobsFacet {
    pub(crate) async_chat_jobs: AsyncChatJobsMap,
}

/// E2E 夹具 `POST /e2e/fixtures/conversation`：仅会话读写（**不含** HTTP/queue/tools）。
///
/// 仅在 **`CM_E2E_FIXTURES=1`** 时挂载；生产勿启用。
#[derive(Clone)]
pub(crate) struct E2eConversationFixtureFacet {
    pub(crate) conversation: AppStateConversationRuntime,
}

impl E2eConversationFixtureFacet {
    pub(crate) async fn delete_conversation_record(&self, conversation_id: &str) {
        self.conversation
            .delete_conversation_record(conversation_id)
            .await
    }

    /// 与 [`AppStateConversationRuntime::save_conversation_messages_if_revision`] 同形（含该处的 `allow` 理由）。
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn save_conversation_messages_if_revision(
        &self,
        conversation_id: String,
        messages: Vec<crate::types::Message>,
        active_agent_role: Option<&str>,
        active_session_mode: Option<&str>,
        active_permission_level: Option<&str>,
        expected_revision: Option<u64>,
        archive_rows: Vec<crate::conversation_store::ArchiveRow>,
    ) -> SaveConversationOutcome {
        self.conversation
            .save_conversation_messages_if_revision(
                conversation_id,
                messages,
                active_agent_role,
                active_session_mode,
                active_permission_level,
                expected_revision,
                archive_rows,
            )
            .await
    }
}

impl WebTasksAppFacet {
    pub(crate) async fn effective_workspace_path(&self) -> String {
        self.http.effective_workspace_path().await
    }
}

impl WebHealthAppFacet {
    pub(crate) async fn effective_workspace_path(&self) -> String {
        self.http.effective_workspace_path().await
    }
}

impl WebStatusAppFacet {
    pub(crate) async fn effective_workspace_path(&self) -> String {
        self.http.effective_workspace_path().await
    }

    pub(crate) async fn conversation_count(&self) -> usize {
        self.conversation.conversation_count().await
    }
}

impl WebChatAppFacet {
    /// 只改会话级 `active_*` 意图（`POST /conversation/state`）。
    ///
    /// `Ok(false)` = 会话不存在；`Err` = 存储层失败（见
    /// [`AppStateConversationRuntime::set_conversation_active_state`]）。
    pub(crate) async fn set_conversation_active_state(
        &self,
        conversation_id: String,
        active_agent_role: Option<&str>,
        active_session_mode: Option<&str>,
        active_permission_level: Option<&str>,
    ) -> Result<bool, String> {
        self.conversation
            .set_conversation_active_state(
                conversation_id,
                active_agent_role,
                active_session_mode,
                active_permission_level,
            )
            .await
    }

    pub(crate) async fn load_conversation_seed(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationTurnSeed> {
        self.conversation
            .load_conversation_seed(conversation_id)
            .await
    }

    /// 读取会话归档原文（`GET /conversation/messages` 带 `include_archive=true`）。
    pub(crate) async fn load_conversation_archive_messages(
        &self,
        conversation_id: &str,
    ) -> Vec<crate::types::Message> {
        self.conversation
            .load_conversation_archive_messages(conversation_id)
            .await
    }

    pub(crate) async fn truncate_conversation_before_user_ordinal_if_revision(
        &self,
        conversation_id: String,
        user_ordinal: usize,
        expected_revision: u64,
    ) -> BranchTruncateOutcome {
        self.conversation
            .truncate_conversation_before_user_ordinal_if_revision(
                conversation_id,
                user_ordinal,
                expected_revision,
            )
            .await
    }

    /// 删除持久化会话行（`DELETE /conversation/{conversation_id}`；不存在时静默成功）。
    pub(crate) async fn delete_conversation_record(&self, conversation_id: &str) {
        self.conversation
            .delete_conversation_record(conversation_id)
            .await
    }

    /// Web：在进程内切换会话存储后端（**不**改写磁盘配置；重启 `serve` 后仍以 TOML 为准）。
    pub(crate) async fn set_web_conversation_store_sqlite(
        &self,
        sqlite: bool,
    ) -> Result<(), String> {
        if sqlite {
            let path = {
                let g = self.cfg.read().await;
                g.conversation_persistence
                    .conversation_store_sqlite_path
                    .clone()
            };
            if path.trim().is_empty() {
                return Err(
                    "未配置 conversation_store_sqlite_path，无法启用 SQLite 会话存储。".into(),
                );
            }
            let new_backing = {
                let conn =
                    open_conversation_sqlite(Path::new(path.trim())).map_err(|e| e.to_string())?;
                ConversationBacking::Sqlite(conn)
            };
            let mut w = self.conversation.conversation_backing.write().await;
            *w = new_backing;
        } else {
            let mut w = self.conversation.conversation_backing.write().await;
            *w = ConversationBacking::memory_default();
        }
        Ok(())
    }
}

impl WebChatTurnAppFacet {
    pub(crate) async fn effective_workspace_path(&self) -> String {
        effective_workspace_path_from_override(&self.workspace_override, &self.cfg).await
    }

    pub(crate) async fn workspace_is_set(&self) -> bool {
        workspace_is_set_from_override(&self.workspace_override).await
    }

    pub(crate) fn next_conversation_id(&self) -> String {
        let n = self
            .conversation
            .conversation_id_counter
            .fetch_add(1, Ordering::Relaxed);
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
        self.conversation
            .load_conversation_seed(conversation_id)
            .await
    }

    pub(crate) fn chat_job_app_facet(&self) -> WebChatJobAppFacet {
        WebChatJobAppFacet {
            conversation: self.conversation.clone(),
            process_handles: self.process_handles.turn_handles_arc(),
            approval_sessions: Arc::clone(&self.approval_sessions),
            tool_job_registry: Arc::clone(&self.tool_job_registry),
        }
    }
}

impl FromRef<Arc<AppState>> for AppStateHttpCore {
    fn from_ref(state: &Arc<AppState>) -> Self {
        state.http.clone()
    }
}

impl FromRef<Arc<AppState>> for ConfigReloadFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            cfg: Arc::clone(&state.http.cfg),
            config_path_for_reload: state.http.config_path_for_reload.clone(),
        }
    }
}

impl FromRef<Arc<AppState>> for UploadsFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            cfg: Arc::clone(&state.http.cfg),
            fallback_uploads_dir: state.http.uploads_dir.clone(),
        }
    }
}

impl FromRef<Arc<AppState>> for WebTasksAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            http: state.http.clone(),
            process_handles: Arc::clone(&state.aux.process_handles),
        }
    }
}

impl FromRef<Arc<AppState>> for WebHealthAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            http: state.http.clone(),
            llm_models_health_cache: Arc::clone(&state.aux.llm_models_health_cache),
        }
    }
}

impl FromRef<Arc<AppState>> for WebStatusAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            http: state.http.clone(),
            conversation: state.conversation.clone(),
            chat_queue: state.chat.chat_queue.clone(),
            long_term_memory: state.aux.long_term_memory.clone(),
            process_handles: Arc::clone(&state.aux.process_handles),
            tiktoken_baseline_cache: Arc::clone(&state.aux.tiktoken_baseline_cache),
        }
    }
}

impl FromRef<Arc<AppState>> for MemoryAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            cfg: Arc::clone(&state.http.cfg),
            long_term_memory: state.aux.long_term_memory.clone(),
        }
    }
}

impl FromRef<Arc<AppState>> for WebChatAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            cfg: Arc::clone(&state.http.cfg),
            conversation: state.conversation.clone(),
            approval_sessions: Arc::clone(&state.aux.approval_sessions),
        }
    }
}

impl FromRef<Arc<AppState>> for WebChatTurnAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            cfg: Arc::clone(&state.http.cfg),
            api_key: Arc::clone(&state.http.api_key),
            client: state.http.client.clone(),
            workspace_override: Arc::clone(&state.http.workspace_override),
            conversation: state.conversation.clone(),
            chat: state.chat.clone(),
            approval_sessions: Arc::clone(&state.aux.approval_sessions),
            process_handles: Arc::clone(&state.aux.process_handles),
            sse_stream_hub: Arc::clone(&state.aux.sse_stream_hub),
            async_chat_jobs: Arc::clone(&state.aux.async_chat_jobs),
            tool_job_registry: Arc::clone(&state.aux.tool_job_registry),
        }
    }
}

impl FromRef<Arc<AppState>> for AsyncChatJobsFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            async_chat_jobs: Arc::clone(&state.aux.async_chat_jobs),
        }
    }
}

impl FromRef<Arc<AppState>> for E2eConversationFixtureFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            conversation: state.conversation.clone(),
        }
    }
}
