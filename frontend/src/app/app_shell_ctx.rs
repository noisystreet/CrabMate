//! 壳层 `*_view` 聚合为 [`AppShellCtx`]，压缩 `App` 内 `view!` 的长实参列表。
//! 阶段 B：对「仅由壳已持有的一组 `RwSignal` 拼出的子组件入参」提供 **`settings_page_form_signals()`** 等组装方法，避免在 **`App`** 中重复罗列字段。
//! **未使用** Leptos `Context` / `<Provider>` 承载整份壳层：状态含 `Rc<RefCell<…>>`、`Rc<dyn Fn()>` 等，
//! 不满足 `provide_context` 的 `Send + Sync + 'static` 约束；以结构体 **`Clone`**（内部多为
//! `Copy` / `Rc::clone` / `Arc::clone`）在 `*_view` 间传递即可。
//! 聊天域中已满足边界的切片见 [`super::shell_runtime_context::ChatShellLeptosContext`]（由 [`super::App`] 根
//! [`provide_context`]，侧栏等用 [`super::shell_runtime_context::expect_chat_shell_ctx`] 读取）。
//!
//! 根壳层 `wire_*` 的**注册顺序**影响隐式时序依赖；见 [`app_shell_wire_phases`](super::app_shell_wire_phases)
//! 与 [`bootstrap_app_shell`](super::app_shell_bootstrap::bootstrap_app_shell)（[`init_app_shell`](super::app_shell_init::init_app_shell)）；聊天列内部顺序见 [`wire_chat_domain`](super::chat::wire_chat_domain)。
//! 聊天域信号切片见 [`ChatDomainWiringSignals`](super::app_signals::ChatDomainWiringSignals)（与 [`AppSignals`] 同源，供接线单点组装）。

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use leptos::html::Div;
use leptos::prelude::*;

use crate::chat_session_state::{ChatSessionSignals, ChatStreamBusyMemos};
use crate::i18n::Locale;
use crate::session_ops::SessionContextAnchor;
use crate::sse_dispatch::ThinkingTraceInfo;

use crate::app_prefs::SidePanelView;

use super::app_signals::AppSignals;
use super::chat::ChatColumnShell;
use super::ide_settings_page::IdeSettingsPageViewInput;
use super::settings_page::{SettingsPageFormSignals, SettingsPageViewInput};
use super::status_tasks_state::StatusTasksSignals;
use super::workspace_panel_state::WorkspacePanelSignals;

use super::approval_modal::ApprovalModalSignals;
use super::changelist_modal::ChangelistModalBodyState;

/// 窄屏顶栏所需句柄（阶段 B：避免向 `mobile_shell_header_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone)]
pub struct MobileShellHeaderSignals {
    pub mobile_nav_open: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub new_session: Rc<dyn Fn()>,
    pub editor_layout_mode: RwSignal<bool>,
}

/// 变更集预览模态所需句柄（阶段 B：避免向 `changelist_modal_view` 传递整份 [`AppShellCtx`]）。
#[derive(Clone, Copy)]
pub struct ChangelistModalSignals {
    pub changelist_modal_open: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub changelist_modal_body: RwSignal<ChangelistModalBodyState>,
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
    pub readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
}

impl SettingsModalSignals {
    /// 与 [`SettingsPageFormSignals::from_app_signals`] 同源 LLM / 外观草稿，另附弹层开关。
    #[must_use]
    pub fn from_app_signals(app: &AppSignals) -> Self {
        let form = SettingsPageFormSignals::from_app_signals(app);
        Self {
            settings_modal: app.modal.settings_modal,
            locale: form.locale,
            theme: form.theme,
            bg_decor: form.bg_decor,
            llm_api_base_draft: form.llm_api_base_draft,
            llm_api_base_preset_select: form.llm_api_base_preset_select,
            llm_model_draft: form.llm_model_draft,
            llm_temperature_draft: form.llm_temperature_draft,
            llm_context_tokens_draft: form.llm_context_tokens_draft,
            llm_thinking_mode_draft: form.llm_thinking_mode_draft,
            llm_api_key_draft: form.llm_api_key_draft,
            llm_has_saved_key: form.llm_has_saved_key,
            llm_settings_feedback: form.llm_settings_feedback,
            executor_llm_api_base_draft: form.executor_llm_api_base_draft,
            executor_llm_api_base_preset_select: form.executor_llm_api_base_preset_select,
            executor_llm_model_draft: form.executor_llm_model_draft,
            executor_llm_api_key_draft: form.executor_llm_api_key_draft,
            executor_llm_has_saved_key: form.executor_llm_has_saved_key,
            executor_llm_settings_feedback: form.executor_llm_settings_feedback,
            execution_mode_draft: form.execution_mode_draft,
            client_llm_storage_tick: form.client_llm_storage_tick,
            readonly_tool_ttl_cache_follow_server: form.readonly_tool_ttl_cache_follow_server,
            saved_model_presets: form.saved_model_presets,
        }
    }
}

/// 会话列表模态所需句柄（阶段 B：避免向 `session_list_modal_view` 传递整份 [`AppShellCtx`]）。
/// 会话 / 草稿 / 语言 / 过滤开关经 [`super::shell_runtime_context::ChatShellLeptosContext`]。
#[derive(Clone, Copy)]
pub struct SessionListModalSignals {
    pub session_modal: RwSignal<bool>,
}

impl SessionListModalSignals {
    #[must_use]
    pub fn from_app_signals(app: &AppSignals) -> Self {
        Self {
            session_modal: app.modal.session_modal,
        }
    }
}

/// 底栏状态条所需句柄（阶段 B：避免向 `status_bar_footer_view` 传递整份 [`AppShellCtx`]）。
/// 会话句柄与界面语言经 [`super::shell_runtime_context::ChatShellLeptosContext`]（角色切换时清流式句柄等）。
#[derive(Clone)]
pub struct StatusBarFooterSignals {
    pub status_bar_visible: RwSignal<bool>,
    pub status_tasks: StatusTasksSignals,
    pub status_err: RwSignal<Option<String>>,
    pub stream_busy_memos: ChatStreamBusyMemos,
    pub status_busy: RwSignal<bool>,
    pub client_llm_storage_tick: RwSignal<u64>,
    pub selected_agent_role: RwSignal<Option<String>>,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
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
/// 会话列表 / 草稿 / 查找后焦点等经 [`super::shell_runtime_context::ChatShellLeptosContext`] 注入子树。
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
    pub session_context_menu: RwSignal<Option<SessionContextAnchor>>,
    pub sidebar_rail_collapsed: RwSignal<bool>,
    pub editor_layout_mode: RwSignal<bool>,
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
    pub chat: ChatSessionSignals,
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
///
/// **Step 1**：扁平字段已并入 [`AppSignals`]；此处仅保留初始化阶段构造的闭包与 [`ChatColumnShell`]，
/// 新增全局 `RwSignal` 时只需改 [`AppSignals`] 与（按需）[`bootstrap_app_shell`](super::app_shell_bootstrap::bootstrap_app_shell) /
/// [`init_app_shell`](super::app_shell_init::init_app_shell) 中的 wire 实参，不必再维护超长「逐字段拷贝」列表。
#[derive(Clone)]
pub struct AppShellCtx {
    pub signals: AppSignals,
    pub new_session: Rc<dyn Fn()>,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub refresh_tasks: Arc<dyn Fn() + Send + Sync>,
    pub toggle_task: Arc<dyn Fn(String) + Send + Sync>,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
    pub insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    pub chat_column: ChatColumnShell,
}

impl AppShellCtx {
    pub fn sidebar_nav_signals(&self) -> SidebarNavSignals {
        SidebarNavSignals {
            locale: self.signals.shell_ui.locale,
            mobile_nav_open: self.signals.sidebar.mobile_nav_open,
            session_modal: self.signals.modal.session_modal,
            new_session: self.new_session.clone(),
            sidebar_session_query: self.signals.sidebar.sidebar_session_query,
            global_message_query: self.signals.sidebar.global_message_query,
            sidebar_search_panel_open: self.signals.sidebar.sidebar_search_panel_open,
            sidebar_rail_ctx_menu: self.signals.sidebar.sidebar_rail_ctx_menu,
            chat_find_panel_open: self.signals.chat_composer.chat_find_panel_open,
            session_context_menu: self.signals.sidebar.session_context_menu,
            sidebar_rail_collapsed: self.signals.sidebar.sidebar_rail_collapsed,
            editor_layout_mode: self.signals.shell_ui.editor_layout_mode,
        }
    }

    pub fn side_column_view_signals(&self) -> SideColumnViewSignals {
        SideColumnViewSignals {
            locale: self.signals.shell_ui.locale,
            side_resize_dragging: self.signals.resize.side_resize_dragging,
            side_panel_view: self.signals.shell_ui.side_panel_view,
            side_width: self.signals.shell_ui.side_width,
            side_resize_session: Rc::clone(&self.signals.resize.side_resize_session),
            side_resize_handles: Rc::clone(&self.signals.resize.side_resize_handles),
            view_menu_open: self.signals.shell_ui.view_menu_open,
            status_bar_visible: self.signals.shell_ui.status_bar_visible,
            settings_page: self.signals.modal.settings_page,
            chat: self.signals.chat,
            workspace_panel: self.signals.to_workspace_panel(),
            status_tasks: self.signals.to_status_tasks(),
            refresh_workspace: Arc::clone(&self.refresh_workspace),
            refresh_tasks: Arc::clone(&self.refresh_tasks),
            toggle_task: Arc::clone(&self.toggle_task),
            changelist_modal_open: self.signals.modal.changelist_modal_open,
            changelist_fetch_nonce: self.signals.modal.changelist_fetch_nonce,
            insert_workspace_file_ref: self.insert_workspace_file_ref,
            thinking_trace_log: self.signals.approval.thinking_trace_log,
        }
    }

    pub fn session_list_modal_signals(&self) -> SessionListModalSignals {
        SessionListModalSignals::from_app_signals(&self.signals)
    }

    pub fn status_bar_footer_signals(&self) -> StatusBarFooterSignals {
        StatusBarFooterSignals {
            status_bar_visible: self.signals.shell_ui.status_bar_visible,
            status_tasks: self.signals.to_status_tasks(),
            status_err: self.signals.stream.status_err,
            stream_busy_memos: self.chat_column.stream_busy_memos,
            status_busy: self.signals.stream.status_busy,
            client_llm_storage_tick: self.signals.llm_settings.client_llm_storage_tick,
            selected_agent_role: self.signals.llm_settings.selected_agent_role,
            refresh_status: Arc::clone(&self.refresh_status),
        }
    }

    pub fn settings_modal_signals(&self) -> SettingsModalSignals {
        SettingsModalSignals::from_app_signals(&self.signals)
    }

    pub fn changelist_modal_signals(&self) -> ChangelistModalSignals {
        ChangelistModalSignals {
            changelist_modal_open: self.signals.modal.changelist_modal_open,
            locale: self.signals.shell_ui.locale,
            changelist_modal_body: self.signals.modal.changelist_modal_body,
            changelist_fetch_nonce: self.signals.modal.changelist_fetch_nonce,
            changelist_body_ref: self.signals.modal.changelist_body_ref,
        }
    }

    pub fn mobile_shell_header_signals(&self) -> MobileShellHeaderSignals {
        MobileShellHeaderSignals {
            mobile_nav_open: self.signals.sidebar.mobile_nav_open,
            locale: self.signals.shell_ui.locale,
            new_session: self.new_session.clone(),
            editor_layout_mode: self.signals.shell_ui.editor_layout_mode,
        }
    }

    pub fn approval_modal_signals(&self) -> ApprovalModalSignals {
        ApprovalModalSignals {
            pending_approval: self.signals.approval.pending_approval,
            locale: self.signals.shell_ui.locale,
        }
    }

    /// 设置页全屏视图：`settings_page` 开关 + 表单信号（阶段 B：单行传入 `SettingsPageView`）。
    pub fn settings_page_view_input(&self) -> SettingsPageViewInput {
        SettingsPageViewInput {
            settings_page: self.signals.modal.settings_page,
            form: self.settings_page_form_signals(),
            status_data: self.signals.status.status_data,
            refresh_status: Arc::clone(&self.refresh_status),
        }
    }

    /// 设置页表单所需 `RwSignal` 聚合（阶段 B：避免在 `App` 的 `view!` 中重复罗列字段）。
    pub fn settings_page_form_signals(&self) -> SettingsPageFormSignals {
        SettingsPageFormSignals::from_app_signals(&self.signals)
    }

    /// IDE 编辑器设置全屏页入参。
    pub fn ide_settings_page_view_input(&self) -> IdeSettingsPageViewInput {
        IdeSettingsPageViewInput {
            ide_settings_page: self.signals.modal.ide_settings_page,
            locale: self.signals.shell_ui.locale,
            editor: self.signals.ide_editor,
        }
    }
}
