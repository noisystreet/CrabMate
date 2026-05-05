//! 壳层 `*_view` 聚合为 [`AppShellCtx`]，压缩 `App` 内 `view!` 的长实参列表。
//! 阶段 B：对「仅由壳已持有的一组 `RwSignal` 拼出的子组件入参」提供 **`settings_page_form_signals()`**、**`chat_find_bar_signals()`** 等组装方法，避免在 **`App`** 中重复罗列字段。
//! **未使用** Leptos `Context` / `<Provider>`：壳层状态含 `Rc<RefCell<…>>`、`Rc<dyn Fn()>` 等，
//! 不满足 `provide_context` 的 `Send + Sync + 'static` 约束；以结构体 **`Clone`**（内部多为
//! `Copy` / `Rc::clone` / `Arc::clone`）在 `*_view` 间传递即可。
//!
//! 根壳层 `wire_*` 的**注册顺序**影响隐式时序依赖；见 [`init_app_shell`](super::app_shell_init::init_app_shell)
//! 与 [`wire_chat_domain`](super::chat::wire_chat_domain) 模块文档。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use leptos::html::Div;
use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::Locale;
use crate::session_ops::SessionContextAnchor;
use crate::sse_dispatch::ThinkingTraceInfo;

use crate::app_prefs::SidePanelView;

use super::chat::{ChatColumnShell, ChatFindBarSignals};
use super::settings_page::{SettingsPageFormSignals, SettingsPageViewInput};
use super::status_tasks_state::StatusTasksSignals;
use super::workspace_panel_state::WorkspacePanelSignals;

use super::approval_modal::ApprovalModalSignals;

/// 窄屏顶栏所需句柄（阶段 B：避免向 `mobile_shell_header_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone)]
pub struct MobileShellHeaderSignals {
    pub mobile_nav_open: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub new_session: Rc<dyn Fn()>,
}

/// 变更集预览模态所需句柄（阶段 B：避免向 `changelist_modal_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone, Copy)]
pub struct ChangelistModalSignals {
    pub changelist_modal_open: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub changelist_modal_loading: RwSignal<bool>,
    pub changelist_modal_err: RwSignal<Option<String>>,
    pub changelist_modal_rev: RwSignal<u64>,
    pub changelist_fetch_nonce: RwSignal<u64>,
    pub changelist_body_ref: NodeRef<Div>,
}

/// 设置弹窗所需句柄（阶段 B：避免向 `settings_modal_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone, Copy)]
pub struct SettingsModalSignals {
    pub settings_modal: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
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
}

/// 会话列表模态所需句柄（阶段 B：避免向 `session_list_modal_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone)]
pub struct SessionListModalSignals {
    pub session_modal: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub chat: ChatSessionSignals,
    pub draft: RwSignal<String>,
    pub apply_assistant_display_filters: RwSignal<bool>,
}

/// 底栏状态条所需句柄（阶段 B：避免向 `status_bar_footer_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone)]
pub struct StatusBarFooterSignals {
    pub status_bar_visible: RwSignal<bool>,
    pub status_tasks: StatusTasksSignals,
    pub status_err: RwSignal<Option<String>>,
    pub tool_busy: RwSignal<bool>,
    pub status_busy: RwSignal<bool>,
    pub client_llm_storage_tick: RwSignal<u64>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub chat: ChatSessionSignals,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
    pub locale: RwSignal<Locale>,
}

type SideResizeHandlesCell = Rc<
    RefCell<
        Option<(
            leptos_dom::helpers::WindowListenerHandle,
            leptos_dom::helpers::WindowListenerHandle,
        )>,
    >,
>;

/// 左侧导航轨所需句柄（阶段 B：避免向 `sidebar_nav_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone)]
pub struct SidebarNavSignals {
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
    pub apply_assistant_display_filters: RwSignal<bool>,
    pub sidebar_rail_collapsed: RwSignal<bool>,
}

/// 右列侧栏所需句柄（阶段 B：避免向 `side_column_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone)]
pub struct SideColumnViewSignals {
    pub locale: RwSignal<Locale>,
    pub side_resize_dragging: RwSignal<bool>,
    pub side_panel_view: RwSignal<SidePanelView>,
    pub side_width: RwSignal<f64>,
    pub side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    pub side_resize_handles: SideResizeHandlesCell,
    pub view_menu_open: RwSignal<bool>,
    pub status_bar_visible: RwSignal<bool>,
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
}

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
    /// 命令审批弹窗（与流式 `ComposerStreamShell` 共用同一 `RwSignal`）。
    pub pending_approval: RwSignal<Option<(String, String, String)>>,
    pub client_llm_storage_tick: RwSignal<u64>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
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
    pub changelist_modal_loading: RwSignal<bool>,
    pub changelist_modal_err: RwSignal<Option<String>>,
    pub changelist_modal_rev: RwSignal<u64>,
    pub changelist_body_ref: NodeRef<Div>,
    pub chat_column: ChatColumnShell,
}

impl AppShellCtx {
    pub fn sidebar_nav_signals(&self) -> SidebarNavSignals {
        SidebarNavSignals {
            locale: self.locale,
            mobile_nav_open: self.mobile_nav_open,
            session_modal: self.session_modal,
            new_session: self.new_session.clone(),
            sidebar_session_query: self.sidebar_session_query,
            global_message_query: self.global_message_query,
            sidebar_search_panel_open: self.sidebar_search_panel_open,
            sidebar_rail_ctx_menu: self.sidebar_rail_ctx_menu,
            chat_find_panel_open: self.chat_find_panel_open,
            chat: self.chat,
            draft: self.draft,
            focus_message_id_after_nav: self.focus_message_id_after_nav,
            session_context_menu: self.session_context_menu,
            apply_assistant_display_filters: self.apply_assistant_display_filters,
            sidebar_rail_collapsed: self.sidebar_rail_collapsed,
        }
    }

    pub fn side_column_view_signals(&self) -> SideColumnViewSignals {
        SideColumnViewSignals {
            locale: self.locale,
            side_resize_dragging: self.side_resize_dragging,
            side_panel_view: self.side_panel_view,
            side_width: self.side_width,
            side_resize_session: Rc::clone(&self.side_resize_session),
            side_resize_handles: Rc::clone(&self.side_resize_handles),
            view_menu_open: self.view_menu_open,
            status_bar_visible: self.status_bar_visible,
            settings_page: self.settings_page,
            workspace_panel: self.workspace_panel,
            status_tasks: self.status_tasks,
            refresh_workspace: Arc::clone(&self.refresh_workspace),
            refresh_tasks: Arc::clone(&self.refresh_tasks),
            toggle_task: Arc::clone(&self.toggle_task),
            changelist_modal_open: self.changelist_modal_open,
            changelist_fetch_nonce: self.changelist_fetch_nonce,
            insert_workspace_file_ref: self.insert_workspace_file_ref,
            thinking_trace_log: self.thinking_trace_log,
        }
    }

    pub fn session_list_modal_signals(&self) -> SessionListModalSignals {
        SessionListModalSignals {
            session_modal: self.session_modal,
            locale: self.locale,
            chat: self.chat,
            draft: self.draft,
            apply_assistant_display_filters: self.apply_assistant_display_filters,
        }
    }

    pub fn status_bar_footer_signals(&self) -> StatusBarFooterSignals {
        StatusBarFooterSignals {
            status_bar_visible: self.status_bar_visible,
            status_tasks: self.status_tasks,
            status_err: self.status_err,
            tool_busy: self.tool_busy,
            status_busy: self.status_busy,
            client_llm_storage_tick: self.client_llm_storage_tick,
            selected_agent_role: self.selected_agent_role,
            chat: self.chat,
            refresh_status: Arc::clone(&self.refresh_status),
            locale: self.locale,
        }
    }

    pub fn settings_modal_signals(&self) -> SettingsModalSignals {
        SettingsModalSignals {
            settings_modal: self.settings_modal,
            locale: self.locale,
            theme: self.theme,
            bg_decor: self.bg_decor,
            llm_api_base_draft: self.llm_api_base_draft,
            llm_api_base_preset_select: self.llm_api_base_preset_select,
            llm_model_draft: self.llm_model_draft,
            llm_temperature_draft: self.llm_temperature_draft,
            llm_context_tokens_draft: self.llm_context_tokens_draft,
            llm_thinking_mode_draft: self.llm_thinking_mode_draft,
            llm_api_key_draft: self.llm_api_key_draft,
            llm_has_saved_key: self.llm_has_saved_key,
            llm_settings_feedback: self.llm_settings_feedback,
            executor_llm_api_base_draft: self.executor_llm_api_base_draft,
            executor_llm_api_base_preset_select: self.executor_llm_api_base_preset_select,
            executor_llm_model_draft: self.executor_llm_model_draft,
            executor_llm_api_key_draft: self.executor_llm_api_key_draft,
            executor_llm_has_saved_key: self.executor_llm_has_saved_key,
            executor_llm_settings_feedback: self.executor_llm_settings_feedback,
            execution_mode_draft: self.execution_mode_draft,
            client_llm_storage_tick: self.client_llm_storage_tick,
        }
    }

    pub fn changelist_modal_signals(&self) -> ChangelistModalSignals {
        ChangelistModalSignals {
            changelist_modal_open: self.changelist_modal_open,
            locale: self.locale,
            changelist_modal_loading: self.changelist_modal_loading,
            changelist_modal_err: self.changelist_modal_err,
            changelist_modal_rev: self.changelist_modal_rev,
            changelist_fetch_nonce: self.changelist_fetch_nonce,
            changelist_body_ref: self.changelist_body_ref,
        }
    }

    pub fn mobile_shell_header_signals(&self) -> MobileShellHeaderSignals {
        MobileShellHeaderSignals {
            mobile_nav_open: self.mobile_nav_open,
            locale: self.locale,
            new_session: self.new_session.clone(),
        }
    }

    pub fn approval_modal_signals(&self) -> ApprovalModalSignals {
        ApprovalModalSignals {
            pending_approval: self.pending_approval,
            locale: self.locale,
        }
    }

    /// 设置页全屏视图：`settings_page` 开关 + 表单信号（阶段 B：单行传入 `SettingsPageView`）。
    pub fn settings_page_view_input(&self) -> SettingsPageViewInput {
        SettingsPageViewInput {
            settings_page: self.settings_page,
            form: self.settings_page_form_signals(),
        }
    }

    /// 主区会话内查找条（阶段 B：避免在 `App` 中逐项传 `RwSignal`）。
    pub fn chat_find_bar_signals(&self) -> ChatFindBarSignals {
        ChatFindBarSignals {
            chat_find_panel_open: self.chat_find_panel_open,
            locale: self.locale,
            chat_find_query: self.chat_column.chat_find_query,
            chat_find_match_ids: self.chat_column.chat_find_match_ids,
            chat_find_cursor: self.chat_column.chat_find_cursor,
            auto_scroll_chat: self.chat_column.auto_scroll_chat,
        }
    }

    /// 设置页表单所需 `RwSignal` 聚合（阶段 B：避免在 `App` 的 `view!` 中重复罗列字段）。
    pub fn settings_page_form_signals(&self) -> SettingsPageFormSignals {
        SettingsPageFormSignals {
            locale: self.locale,
            theme: self.theme,
            bg_decor: self.bg_decor,
            llm_api_base_draft: self.llm_api_base_draft,
            llm_api_base_preset_select: self.llm_api_base_preset_select,
            llm_model_draft: self.llm_model_draft,
            llm_temperature_draft: self.llm_temperature_draft,
            llm_context_tokens_draft: self.llm_context_tokens_draft,
            llm_thinking_mode_draft: self.llm_thinking_mode_draft,
            llm_api_key_draft: self.llm_api_key_draft,
            llm_has_saved_key: self.llm_has_saved_key,
            llm_settings_feedback: self.llm_settings_feedback,
            executor_llm_api_base_draft: self.executor_llm_api_base_draft,
            executor_llm_api_base_preset_select: self.executor_llm_api_base_preset_select,
            executor_llm_model_draft: self.executor_llm_model_draft,
            executor_llm_api_key_draft: self.executor_llm_api_key_draft,
            executor_llm_has_saved_key: self.executor_llm_has_saved_key,
            executor_llm_settings_feedback: self.executor_llm_settings_feedback,
            execution_mode_draft: self.execution_mode_draft,
            client_llm_storage_tick: self.client_llm_storage_tick,
        }
    }
}
