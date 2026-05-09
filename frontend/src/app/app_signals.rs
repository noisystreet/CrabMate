//! `App` 级响应式信号的**按域聚合**。
//!
//! 减少 `App` 中平铺的 40+ `RwSignal` 声明，使 `App` 更贴近「声明信号 + 调用 `wire_*`」的壳层职责。
//! 已有的聚合类型：[`ChatSessionSignals`](crate::chat_session_state::ChatSessionSignals)、
//! [`StatusTasksSignals`](super::status_tasks_state::StatusTasksSignals)、
//! [`WorkspacePanelSignals`](super::workspace_panel_state::WorkspacePanelSignals)。

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;
use leptos_dom::helpers::WindowListenerHandle;

use crate::api::{StatusData, TasksData, WorkspaceData, load_saved_model_presets_from_storage};
use crate::app::shell_prefs_storage;
use crate::app_prefs::{SIDEBAR_RAIL_COLLAPSED_KEY, SidePanelView, load_bool_key};
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::Locale;
use crate::session_ops::SessionContextAnchor;
use crate::session_sync::SessionSyncState;
use crate::sse_dispatch::ThinkingTraceInfo;

pub use super::status_tasks_state::StatusTasksSignals;
pub use super::workspace_panel_state::WorkspacePanelSignals;
pub use crate::chat_session_state::ChatSessionSignals;

#[derive(Clone, Copy)]
pub struct ShellUISignals {
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub view_menu_open: RwSignal<bool>,
    pub status_bar_visible: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub side_width: RwSignal<f64>,
    pub web_ui_config_loaded: RwSignal<bool>,
    pub markdown_render: RwSignal<bool>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

impl ShellUISignals {
    pub fn new() -> Self {
        let s = shell_prefs_storage::read_shell_ui_initial_snapshot();
        Self {
            theme: RwSignal::new(s.theme),
            bg_decor: RwSignal::new(s.bg_decor),
            locale: RwSignal::new(s.locale),
            view_menu_open: RwSignal::new(false),
            status_bar_visible: RwSignal::new(s.status_bar_visible),
            side_panel_view: RwSignal::new(s.side_panel_view),
            side_width: RwSignal::new(s.side_width),
            web_ui_config_loaded: RwSignal::new(false),
            markdown_render: RwSignal::new(true),
            apply_assistant_display_filters: RwSignal::new(true),
        }
    }
}

impl Default for ShellUISignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ChatComposerSignals {
    pub draft: RwSignal<String>,
    pub pending_images: RwSignal<Vec<String>>,
    /// 输入框镜像层 HTML（`@{工作区路径}` 高亮）；与草稿缓冲同源更新。
    pub composer_mirror_html: RwSignal<String>,
    pub composer_mirror_scroll_top: RwSignal<f64>,
    pub composer_input_ref: NodeRef<leptos::html::Textarea>,
    pub collapsed_long_assistant_ids: RwSignal<Vec<String>>,
    /// 连续工具组中用户手动**收起**为仅显示最后一条的分组 head（message id）；默认空 = 全部展开。
    pub collapsed_tool_run_heads: RwSignal<HashSet<String>>,
    pub auto_scroll_chat: RwSignal<bool>,
    pub messages_scroll_from_effect: RwSignal<bool>,
    pub last_messages_scroll_top: RwSignal<i32>,
    pub messages_scroller: NodeRef<Div>,
    pub timeline_panel_expanded: RwSignal<bool>,
    pub chat_find_query: RwSignal<String>,
    pub chat_find_match_ids: RwSignal<Vec<String>>,
    pub chat_find_cursor: RwSignal<usize>,
    pub chat_find_panel_open: RwSignal<bool>,
    pub focus_message_id_after_nav: RwSignal<Option<String>>,
}

impl ChatComposerSignals {
    pub fn new() -> Self {
        Self {
            draft: RwSignal::new(String::new()),
            pending_images: RwSignal::new(Vec::new()),
            composer_mirror_html: RwSignal::new(String::new()),
            composer_mirror_scroll_top: RwSignal::new(0.0),
            composer_input_ref: NodeRef::new(),
            collapsed_long_assistant_ids: RwSignal::new(Vec::new()),
            collapsed_tool_run_heads: RwSignal::new(HashSet::new()),
            auto_scroll_chat: RwSignal::new(true),
            messages_scroll_from_effect: RwSignal::new(false),
            last_messages_scroll_top: RwSignal::new(0),
            messages_scroller: NodeRef::new(),
            timeline_panel_expanded: RwSignal::new(
                crate::app::chat::load_timeline_panel_expanded_default(),
            ),
            chat_find_query: RwSignal::new(String::new()),
            chat_find_match_ids: RwSignal::new(Vec::new()),
            chat_find_cursor: RwSignal::new(0),
            chat_find_panel_open: RwSignal::new(false),
            focus_message_id_after_nav: RwSignal::new(None),
        }
    }
}

impl Default for ChatComposerSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct ApprovalSignals {
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub approval_expanded: RwSignal<bool>,
    pub last_approval_sid: RwSignal<String>,
    pub pending_clarification: RwSignal<Option<PendingClarificationForm>>,
    pub thinking_trace_log: RwSignal<Vec<ThinkingTraceInfo>>,
}

impl ApprovalSignals {
    pub fn new() -> Self {
        Self {
            pending_approval: RwSignal::new(None),
            approval_expanded: RwSignal::new(false),
            last_approval_sid: RwSignal::new(String::new()),
            pending_clarification: RwSignal::new(None),
            thinking_trace_log: RwSignal::new(Vec::new()),
        }
    }
}

impl Default for ApprovalSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct StreamControlSignals {
    pub status_busy: RwSignal<bool>,
    pub status_err: RwSignal<Option<String>>,
    pub tool_busy: RwSignal<bool>,
    pub abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub user_cancelled_stream: Arc<Mutex<bool>>,
}

impl StreamControlSignals {
    pub fn new() -> Self {
        Self {
            status_busy: RwSignal::new(false),
            status_err: RwSignal::new(None),
            tool_busy: RwSignal::new(false),
            abort_cell: Arc::new(Mutex::new(None)),
            user_cancelled_stream: Arc::new(Mutex::new(false)),
        }
    }
}

impl Default for StreamControlSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct SidebarSignals {
    pub sidebar_rail_collapsed: RwSignal<bool>,
    pub sidebar_session_query: RwSignal<String>,
    pub global_message_query: RwSignal<String>,
    pub sidebar_search_panel_open: RwSignal<bool>,
    pub sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub mobile_nav_open: RwSignal<bool>,
}

impl SidebarSignals {
    pub fn new() -> Self {
        Self {
            sidebar_rail_collapsed: RwSignal::new(load_bool_key(SIDEBAR_RAIL_COLLAPSED_KEY, false)),
            sidebar_session_query: RwSignal::new(String::new()),
            global_message_query: RwSignal::new(String::new()),
            sidebar_search_panel_open: RwSignal::new(false),
            sidebar_rail_ctx_menu: RwSignal::new(None),
            session_context_menu: RwSignal::new(None),
            mobile_nav_open: RwSignal::new(false),
        }
    }
}

impl Default for SidebarSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct ModalSignals {
    pub session_modal: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub settings_page: RwSignal<bool>,
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_modal_loading: RwSignal<bool>,
    pub changelist_modal_err: RwSignal<Option<String>>,
    pub changelist_modal_html: RwSignal<String>,
    pub changelist_modal_rev: RwSignal<u64>,
    pub changelist_body_ref: NodeRef<Div>,
    pub changelist_fetch_nonce: RwSignal<u64>,
}

impl ModalSignals {
    pub fn new() -> Self {
        Self {
            session_modal: RwSignal::new(false),
            settings_modal: RwSignal::new(false),
            settings_page: RwSignal::new(false),
            changelist_modal_open: RwSignal::new(false),
            changelist_modal_loading: RwSignal::new(false),
            changelist_modal_err: RwSignal::new(None),
            changelist_modal_html: RwSignal::new(String::new()),
            changelist_modal_rev: RwSignal::new(0),
            changelist_body_ref: NodeRef::new(),
            changelist_fetch_nonce: RwSignal::new(0),
        }
    }
}

impl Default for ModalSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct LLMSettingsSignals {
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
    pub execution_mode_draft: RwSignal<String>,
    pub client_llm_storage_tick: RwSignal<u64>,
    /// **`true`**：聊天请求不附带 **`readonly_tool_ttl_cache_secs`**，跟随服务端；**`false`**：附带 **`0`** 关闭只读 **`run_command`** 短时缓存。
    pub readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
    pub selected_agent_role: RwSignal<Option<String>>,
    /// 本机已保存的多条模型预设（与扁平 `client_llm` 并存；用于设置页下拉选用）。
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
}

impl LLMSettingsSignals {
    pub fn new() -> Self {
        Self {
            llm_api_base_draft: RwSignal::new(String::new()),
            llm_api_base_preset_select: RwSignal::new(String::from("server")),
            llm_model_draft: RwSignal::new(String::new()),
            llm_temperature_draft: RwSignal::new(String::new()),
            llm_context_tokens_draft: RwSignal::new(String::new()),
            llm_thinking_mode_draft: RwSignal::new("server".to_string()),
            llm_api_key_draft: RwSignal::new(String::new()),
            llm_has_saved_key: RwSignal::new(false),
            llm_settings_feedback: RwSignal::new(None),
            executor_llm_api_base_draft: RwSignal::new(String::new()),
            executor_llm_api_base_preset_select: RwSignal::new(String::from("server")),
            executor_llm_model_draft: RwSignal::new(String::new()),
            executor_llm_api_key_draft: RwSignal::new(String::new()),
            executor_llm_has_saved_key: RwSignal::new(false),
            executor_llm_settings_feedback: RwSignal::new(None),
            execution_mode_draft: RwSignal::new("rolling_planning".to_string()),
            client_llm_storage_tick: RwSignal::new(0),
            readonly_tool_ttl_cache_follow_server: RwSignal::new(
                crate::api::load_readonly_tool_ttl_cache_follow_server_from_storage(),
            ),
            selected_agent_role: RwSignal::new(shell_prefs_storage::read_agent_role_initial()),
            saved_model_presets: RwSignal::new(load_saved_model_presets_from_storage()),
        }
    }
}

impl Default for LLMSettingsSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct ResizeSignals {
    pub side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    pub side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>>,
    pub side_resize_dragging: RwSignal<bool>,
}

impl ResizeSignals {
    pub fn new() -> Self {
        Self {
            side_resize_session: Rc::new(RefCell::new(None)),
            side_resize_handles: Rc::new(RefCell::new(None)),
            side_resize_dragging: RwSignal::new(false),
        }
    }
}

impl Default for ResizeSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct WorkspaceSignals {
    pub workspace_data: RwSignal<Option<WorkspaceData>>,
    pub workspace_subtree_expanded: RwSignal<HashSet<String>>,
    pub workspace_subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    pub workspace_subtree_loading: RwSignal<HashSet<String>>,
    pub workspace_err: RwSignal<Option<String>>,
    pub workspace_loading: RwSignal<bool>,
    pub workspace_path_draft: RwSignal<String>,
    pub workspace_set_err: RwSignal<Option<String>>,
    pub workspace_set_busy: RwSignal<bool>,
    pub workspace_pick_busy: RwSignal<bool>,
}

impl WorkspaceSignals {
    pub fn new() -> Self {
        Self {
            workspace_data: RwSignal::new(None),
            workspace_subtree_expanded: RwSignal::new(HashSet::new()),
            workspace_subtree_cache: RwSignal::new(HashMap::new()),
            workspace_subtree_loading: RwSignal::new(HashSet::new()),
            workspace_err: RwSignal::new(None),
            workspace_loading: RwSignal::new(false),
            workspace_path_draft: RwSignal::new(String::new()),
            workspace_set_err: RwSignal::new(None),
            workspace_set_busy: RwSignal::new(false),
            workspace_pick_busy: RwSignal::new(false),
        }
    }
}

impl Default for WorkspaceSignals {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy)]
pub struct StatusSignals {
    pub status_data: RwSignal<Option<StatusData>>,
    pub status_loading: RwSignal<bool>,
    pub status_fetch_err: RwSignal<Option<String>>,
    pub tasks_data: RwSignal<TasksData>,
    pub tasks_err: RwSignal<Option<String>>,
    pub tasks_loading: RwSignal<bool>,
}

impl StatusSignals {
    pub fn new() -> Self {
        Self {
            status_data: RwSignal::new(None),
            status_loading: RwSignal::new(true),
            status_fetch_err: RwSignal::new(None),
            tasks_data: RwSignal::new(TasksData { items: vec![] }),
            tasks_err: RwSignal::new(None),
            tasks_loading: RwSignal::new(false),
        }
    }
}

impl Default for StatusSignals {
    fn default() -> Self {
        Self::new()
    }
}

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
    pub llm_settings: LLMSettingsSignals,
    pub resize: ResizeSignals,
    pub workspace: WorkspaceSignals,
    pub status: StatusSignals,
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
            stream_job_id: RwSignal::new(None),
            stream_last_event_seq: RwSignal::new(0),
            stream_bound_session_id: RwSignal::new(None),
            reasoning_preserved: RwSignal::new(HashMap::new()),
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
            llm_settings,
            resize,
            workspace,
            status,
        }
    }

    pub fn to_status_tasks(&self) -> StatusTasksSignals {
        StatusTasksSignals {
            status_data: self.status.status_data,
            status_loading: self.status.status_loading,
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
    /// 从整份 [`AppSignals`] 抽取聊天域接线子集（与 [`ComposerStreamShell::from_app_signals`] 对称的「单点组装」）。
    #[must_use]
    pub fn from_app_signals(app: &AppSignals) -> Self {
        Self {
            initialized: app.initialized,
            chat: app.chat,
            composer: app.chat_composer.clone(),
            pending_clarification: app.approval.pending_clarification,
            locale: app.shell_ui.locale,
            apply_assistant_display_filters: app.shell_ui.apply_assistant_display_filters,
            selected_agent_role: app.llm_settings.selected_agent_role,
        }
    }
}
