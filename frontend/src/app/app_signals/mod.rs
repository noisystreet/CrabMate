//! `App` 级响应式信号的**按域聚合**。
//!
//! 减少 `App` 中平铺的 40+ `RwSignal` 声明，使 `App` 更贴近「声明信号 + 调用 `wire_*`」的壳层职责。
//! 子模块按域拆分文件；[`AppSignals`] 与 [`ChatDomainWiringSignals`] 仍在本 `mod` 根聚合导出。
//! 已有的聚合类型：[`ChatSessionSignals`](crate::chat_session_state::ChatSessionSignals)、
//! [`StatusTasksSignals`](super::status_tasks_state::StatusTasksSignals)、
//! [`WorkspacePanelSignals`](super::workspace_panel_state::WorkspacePanelSignals)。

mod approval;
mod chat_composer;
mod ide_editor;
mod llm_settings;
mod modal;
mod resize;
mod shell_ui;
mod sidebar;
mod status;
mod stream_control;
mod workspace;

use std::collections::HashMap;

use leptos::prelude::*;

pub use approval::ApprovalSignals;
pub use chat_composer::ChatComposerSignals;
pub use ide_editor::IdeEditorSignals;
pub use llm_settings::LLMSettingsSignals;
pub use modal::ModalSignals;
pub use resize::ResizeSignals;
pub use shell_ui::ShellUISignals;
pub use sidebar::SidebarSignals;
pub use status::StatusSignals;
pub use stream_control::StreamControlSignals;
pub use workspace::WorkspaceSignals;

pub use super::status_tasks_state::StatusTasksSignals;
pub use super::workspace_panel_state::WorkspacePanelSignals;
pub use crate::chat_session_state::ChatSessionSignals;

use crate::clarification_form::PendingClarificationForm;
use crate::i18n::Locale;
use crate::session_sync::SessionSyncState;
use crate::storage::SESSIONS_KEY_LEGACY;

#[derive(Clone)]
pub struct AppSignals {
    pub initialized: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub shell_ui: ShellUISignals,
    pub chat_composer: ChatComposerSignals,
    pub approval: ApprovalSignals,
    pub stream: StreamControlSignals,
    pub sidebar: SidebarSignals,
    pub modal: ModalSignals,
    pub ide_editor: IdeEditorSignals,
    pub llm_settings: LLMSettingsSignals,
    pub resize: ResizeSignals,
    pub workspace: WorkspaceSignals,
    pub status: StatusSignals,
    /// 当前写入 `localStorage` 的会话 JSON 键（随工作区分桶变化）。
    pub session_sessions_storage_key: RwSignal<String>,
}

impl AppSignals {
    /// 聊天域接线用信号切片（[`ChatDomainWiringSignals`]）；含一份 [`ChatComposerSignals`] 克隆，**仅**宜在初始化等低频路径调用。
    #[inline]
    #[must_use]
    pub fn chat_domain_wiring(&self) -> ChatDomainWiringSignals {
        ChatDomainWiringSignals::from_app_signals(self)
    }

    pub fn new() -> Self {
        let shell_ui = ShellUISignals::new();
        let chat_composer = ChatComposerSignals::new();
        let approval = ApprovalSignals::new();
        let stream = StreamControlSignals::new();
        let sidebar = SidebarSignals::new();
        let modal = ModalSignals::new();
        let ide_editor = IdeEditorSignals::new();
        let llm_settings = LLMSettingsSignals::new();
        let resize = ResizeSignals::new();
        let workspace = WorkspaceSignals::new();
        let status = StatusSignals::new();

        let session_sync = RwSignal::new(SessionSyncState::local_only());
        let chat = ChatSessionSignals {
            sessions: RwSignal::new(Vec::new()),
            active_id: RwSignal::new(String::new()),
            session_sync,
            session_hydrate_nonce: RwSignal::new(0),
            stream_transport: RwSignal::new(
                crate::chat_session_state::ChatStreamTransport::default(),
            ),
            stream_last_sse_event_seq: RwSignal::new(0),
            reasoning_preserved: RwSignal::new(HashMap::new()),
            stream_text_overlay: RwSignal::new(None),
            conversation_prompt_tokens: RwSignal::new(None),
        };

        Self {
            initialized: RwSignal::new(false),
            chat,
            shell_ui,
            chat_composer,
            approval,
            stream,
            sidebar,
            modal,
            ide_editor,
            llm_settings,
            resize,
            workspace,
            status,
            session_sessions_storage_key: RwSignal::new(SESSIONS_KEY_LEGACY.to_string()),
        }
    }

    pub fn to_status_tasks(&self) -> StatusTasksSignals {
        StatusTasksSignals {
            status_data: self.status.status_data,
            status_loading: self.status.status_loading,
            status_fetch_phase: self.status.status_fetch_phase,
            status_fetch_err: self.status.status_fetch_err,
            tasks_data: self.status.tasks_data,
            tasks_err: self.status.tasks_err,
            tasks_loading: self.status.tasks_loading,
        }
    }

    pub fn to_workspace_panel(&self) -> WorkspacePanelSignals {
        WorkspacePanelSignals {
            workspace_data: self.workspace.workspace_data,
            workspace_subtree_expanded: self.workspace.workspace_subtree_expanded,
            workspace_subtree_cache: self.workspace.workspace_subtree_cache,
            workspace_subtree_loading: self.workspace.workspace_subtree_loading,
            workspace_err: self.workspace.workspace_err,
            workspace_loading: self.workspace.workspace_loading,
            workspace_path_draft: self.workspace.workspace_path_draft,
            workspace_set_err: self.workspace.workspace_set_err,
            workspace_set_busy: self.workspace.workspace_set_busy,
            workspace_pick_busy: self.workspace.workspace_pick_busy,
        }
    }
}

impl Default for AppSignals {
    fn default() -> Self {
        Self::new()
    }
}

/// 聊天列 `wire_chat_domain_effects` 所需信号的**单点切片**（[`AppSignals`] 子域组合）。
///
/// 目的：壳层初始化只调用 [`ChatDomainWiringSignals::from_app_signals`]，避免向
/// [`super::chat::wire_chat_domain::WireChatDomainEffectsArgs`] 重复传入与 [`ChatSessionSignals`] 同源的
/// `sessions` / `active_id`；接线内部一律经 [`Self::chat`] 访问会话向量与活动 id。
#[derive(Clone)]
pub struct ChatDomainWiringSignals {
    pub initialized: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub composer: ChatComposerSignals,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    pub locale: RwSignal<Locale>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub selected_agent_role: RwSignal<Option<String>>,
}

impl ChatDomainWiringSignals {
    /// 从整份 [`AppSignals`] 抽取聊天域接线子集（与 [`crate::app::chat::ComposerStreamShell::from_app_signals`] 对称的「单点组装」）。
    #[must_use]
    pub fn from_app_signals(app: &AppSignals) -> Self {
        Self {
            initialized: app.initialized,
            chat: app.chat,
            composer: app.chat_composer,
            pending_clarification: app.approval.pending_clarification,
            locale: app.shell_ui.locale,
            apply_assistant_display_filters: app.shell_ui.apply_assistant_display_filters,
            selected_agent_role: app.llm_settings.selected_agent_role,
        }
    }
}
