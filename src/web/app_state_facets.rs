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
use crate::conversation_store::SaveConversationOutcome;
use crate::health::CachedLlmModelsHealthProbe;
use crate::process_handles::ProcessHandles;
use crate::sse::SseStreamHub;
use crate::web::async_chat_job::AsyncChatJobsMap;

use crate::chat_job_queue::ChatJobQueue;
use crate::memory::long_term_memory::LongTermMemoryRuntime;

use super::app_state::{
    AppState, AppStateChatRuntime, AppStateConversationRuntime, AppStateHttpCore,
    ApprovalSessionSlot, ConversationBacking, ConversationTurnSeed, WebChatJobAppFacet,
    effective_workspace_path_from_override, open_conversation_sqlite,
    workspace_is_set_from_override,
};

/// `POST /config/reload`：共享配置 + 重载路径。
#[derive(Clone)]
pub(crate) struct ConfigReloadFacet {
    pub(crate) cfg: SharedAgentConfig,
    pub(crate) config_path_for_reload: Option<String>,
}

/// `POST /upload` / `POST /upload/delete`：uploads 目录。
#[derive(Clone)]
pub(crate) struct UploadsFacet {
    pub(crate) uploads_dir: PathBuf,
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
}

/// 仅 `GET` 异步任务状态：只持 `async_chat_jobs`。
#[derive(Clone)]
pub(crate) struct AsyncChatJobsFacet {
    pub(crate) async_chat_jobs: AsyncChatJobsMap,
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
    pub(crate) async fn load_conversation_seed(
        &self,
        conversation_id: &str,
    ) -> Option<ConversationTurnSeed> {
        self.conversation
            .load_conversation_seed(conversation_id)
            .await
    }

    pub(crate) async fn truncate_conversation_before_user_ordinal_if_revision(
        &self,
        conversation_id: String,
        user_ordinal: usize,
        expected_revision: u64,
    ) -> SaveConversationOutcome {
        self.conversation
            .truncate_conversation_before_user_ordinal_if_revision(
                conversation_id,
                user_ordinal,
                expected_revision,
            )
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
            uploads_dir: state.http.uploads_dir.clone(),
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
