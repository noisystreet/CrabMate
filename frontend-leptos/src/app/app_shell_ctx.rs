//! 壳层 `*_view` 聚合为 [`AppShellCtx`]，压缩 `App` 内 `view!` 的长实参列表。
//!
//! **未使用** Leptos `Context` / `<Provider>`：壳层状态含 `Rc<RefCell<…>>`、`Rc<dyn Fn()>` 等，
//! 不满足 `provide_context` 的 `Send + Sync + 'static` 约束；以结构体 **`Clone`**（内部多为
//! `Copy` / `Rc::clone` / `Arc::clone`）在 `*_view` 间传递即可。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use leptos::html::Div;
use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::SessionContextAnchor;
use crate::sse_dispatch::ThinkingTraceInfo;

use crate::app_prefs::SidePanelView;

use super::chat::ChatColumnShell;
use super::status_tasks_state::StatusTasksSignals;
use super::workspace_panel_state::WorkspacePanelSignals;

type SideResizeHandlesCell = Rc<
    RefCell<
        Option<(
            leptos_dom::helpers::WindowListenerHandle,
            leptos_dom::helpers::WindowListenerHandle,
        )>,
    >,
>;

/// 根壳 `App` 与侧栏、底栏、各模态之间共享的一组句柄（由 `App` 构造一次，按需 `clone()`）。
#[derive(Clone)]
pub struct AppShellCtx {
    pub locale: RwSignal<Locale>,
    pub mobile_nav_open: RwSignal<bool>,
    pub session_modal: RwSignal<bool>,
    pub new_session: Rc<dyn Fn()>,
    pub sidebar_session_query: RwSignal<String>,
    pub global_message_query: RwSignal<String>,
    pub sidebar_search_panel_open: RwSignal<bool>,
    pub sidebar_rail_ctx_menu: RwSignal<Option<(f64, f64)>>,
    pub chat_find_panel_open: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub draft: RwSignal<String>,
    pub focus_message_id_after_nav: RwSignal<Option<String>>,
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub composer_draft_buffer: Arc<Mutex<String>>,
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub sidebar_rail_collapsed: RwSignal<bool>,
    pub side_resize_dragging: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub side_width: RwSignal<f64>,
    pub side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    pub side_resize_handles: SideResizeHandlesCell,
    pub view_menu_open: RwSignal<bool>,
    pub status_bar_visible: RwSignal<bool>,
    pub settings_modal: RwSignal<bool>,
    pub settings_page: RwSignal<bool>,
    pub workspace_panel: WorkspacePanelSignals,
    pub status_tasks: StatusTasksSignals,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub refresh_tasks: Arc<dyn Fn() + Send + Sync>,
    pub toggle_task: Arc<dyn Fn(String) + Send + Sync>,
    pub changelist_modal_open: RwSignal<bool>,
    pub changelist_fetch_nonce: RwSignal<u64>,
    pub insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    pub thinking_trace_log: RwSignal<Vec<ThinkingTraceInfo>>,
    pub status_err: RwSignal<Option<String>>,
    pub tool_busy: RwSignal<bool>,
    pub status_busy: RwSignal<bool>,
    pub client_llm_storage_tick: RwSignal<u64>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub context_used_estimate: RwSignal<usize>,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
    pub changelist_modal_loading: RwSignal<bool>,
    pub changelist_modal_err: RwSignal<Option<String>>,
    pub changelist_modal_rev: RwSignal<u64>,
    pub changelist_body_ref: NodeRef<Div>,
    pub chat_column: ChatColumnShell,
}
