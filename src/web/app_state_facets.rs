//! Handler 侧 AppState 投影（axum `FromRef`），避免窄路由持有整包 [`AppState`]。
//!
//! 与队列侧 [`WebChatJobAppFacet`](super::app_state::WebChatJobAppFacet) 同族；见 `docs/design/web_host_extract.md`。

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::FromRef;

use crate::config::SharedAgentConfig;
use crate::health::CachedLlmModelsHealthProbe;
use crate::process_handles::ProcessHandles;

use super::app_state::{
    AppState, AppStateChatRuntime, AppStateConversationRuntime, AppStateHttpCore, AppStateWebAux,
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

/// `GET|PUT /tasks`：工作区键 + 侧栏任务表（完整 [`ProcessHandles`]）。
#[derive(Clone)]
pub(crate) struct WebTasksAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) process_handles: Arc<ProcessHandles>,
}

/// `GET /health`：HTTP 核 + 模型探测缓存。
#[derive(Clone)]
pub(crate) struct WebHealthAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) llm_models_health_cache: Arc<std::sync::Mutex<Option<CachedLlmModelsHealthProbe>>>,
}

/// `GET /status`：配置/工具/队列/会话/LTM 等只读快照所需面。
#[derive(Clone)]
pub(crate) struct WebStatusAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) conversation: AppStateConversationRuntime,
    pub(crate) chat: AppStateChatRuntime,
    pub(crate) aux: AppStateWebAux,
}

/// `GET /workspace/changelog`：配置开关 + 变更集注册表。
#[derive(Clone)]
pub(crate) struct WebChangelogAppFacet {
    pub(crate) http: AppStateHttpCore,
    pub(crate) process_handles: Arc<ProcessHandles>,
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
            chat: state.chat.clone(),
            aux: state.aux.clone(),
        }
    }
}

impl FromRef<Arc<AppState>> for WebChangelogAppFacet {
    fn from_ref(state: &Arc<AppState>) -> Self {
        Self {
            http: state.http.clone(),
            process_handles: Arc::clone(&state.aux.process_handles),
        }
    }
}
