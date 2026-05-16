//! App 壳层 `wire_*` 的**分阶段**注册。
//!
//! **唯一对外入口**：[`run_shell_wiring_in_order`]（由 [`super::app_shell_bootstrap::bootstrap_app_shell`] 调用）。
//! 阶段子函数均为本模块**私有**，避免在其它模块零散调用导致顺序漂移。
//!
//! | 阶段 | 职责 |
//! |------|------|
//! | 1 | 会话生命周期、首启与水合相关 `Effect`（须最先）。 |
//! | 2 | 本机偏好持久化、审批条展开跟随、主题/语言/bg DOM、设置打开时 LLM 草稿。 |
//! | 3 | `Escape` 分层关闭；当前选中会话 **`Delete` / `Shift+Delete`** 删除（依赖阶段 2 已挂接的弹层/菜单信号）。 |
//! | 4a | 工作区 / 变更集域（依赖 `refresh_workspace`）。 |
//! | 4b | `/status`、`/tasks` 与侧栏任务面可见时的 `Effect`。 |
//! | 4c | 工作区路径插入 composer、流式壳、聊天列 `wire_*`（与 4a 共用 `refresh_workspace`）。 |
//!
//! 聊天主列内部顺序见 [`super::chat::wire_chat_domain`]。

use std::sync::Arc;

use leptos::prelude::*;

use super::app_shell_effects::{
    SessionDeleteHotkeySignals, ShellEscapeSignals, WireSettingsModalLlmDraftsSignals,
    wire_approval_expanded_follows_pending, wire_close_shell_chrome_when_ide_layout,
    wire_collapse_sidebar_rail_when_ide_layout, wire_escape_key_layered_dismiss,
    wire_persist_agent_role, wire_persist_editor_layout_mode, wire_persist_ide_editor_prefs,
    wire_persist_side_panel_view_flags, wire_persist_side_width,
    wire_persist_sidebar_rail_collapsed, wire_persist_status_bar_visible,
    wire_session_delete_hotkey, wire_settings_modal_llm_drafts_on_open,
    wire_sync_bg_decor_to_storage_and_dom, wire_sync_ide_layout_dom_and_tauri_chrome,
    wire_sync_locale_html_lang, wire_sync_theme_to_storage_and_dom,
};
use super::app_signals::AppSignals;
use super::chat::ChatComposerWires;
use super::chat::{
    ComposerStreamShell,
    wire_chat_domain::{WireChatDomainEffectsArgs, wire_chat_domain_effects},
    wire_chat_session_lifecycle::{
        WireChatSessionLifecycleEffectsArgs, wire_chat_session_lifecycle_effects,
    },
};
use super::status_tasks_wiring::{
    make_refresh_status, make_refresh_tasks, make_toggle_task, wire_status_tasks_domain_effects,
};
use super::wire_workspace_domain::{WireWorkspaceDomainEffectsArgs, wire_workspace_domain_effects};
use super::workspace_panel::{make_insert_workspace_path_into_composer, make_refresh_workspace};
use crate::chat_session_state::ChatStreamBusyMemos;

type RefreshWorkspaceFn = Arc<dyn Fn() + Send + Sync>;
type RefreshStatusFn = Arc<dyn Fn() + Send + Sync>;
type RefreshTasksFn = Arc<dyn Fn() + Send + Sync>;
type ToggleTaskFn = Arc<dyn Fn(String) + Send + Sync>;
type InsertWorkspacePathFn = Arc<dyn Fn(String) + Send + Sync>;

/// 阶段 4b：`/status`、`/tasks` 与任务切换闭包。
struct StatusTasksSpawn {
    refresh_status: RefreshStatusFn,
    refresh_tasks: RefreshTasksFn,
    toggle_task: ToggleTaskFn,
}

/// 阶段 4c：工作区路径插入、流式壳与聊天列 `wire_*` 产物。
struct ChatColumnWiringPack {
    insert_workspace_file_ref: StoredValue<InsertWorkspacePathFn>,
    chat_stream_shell: ComposerStreamShell,
    chat_wires: ChatComposerWires,
    stream_busy_memos: ChatStreamBusyMemos,
}

/// 阶段 4 尾部产物（不含 `refresh_workspace`，其由调用方持有并传入 4a/4c）。
struct Phase4WiringTail {
    refresh_status: RefreshStatusFn,
    refresh_tasks: RefreshTasksFn,
    toggle_task: ToggleTaskFn,
    insert_workspace_file_ref: StoredValue<InsertWorkspacePathFn>,
    chat_stream_shell: ComposerStreamShell,
    chat_wires: ChatComposerWires,
    stream_busy_memos: ChatStreamBusyMemos,
}

/// [`run_shell_wiring_in_order`] 的解构产物；[`super::app_shell_bootstrap::bootstrap_app_shell`] 唯一消费。
pub(super) struct ShellWiringOutput {
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
    pub refresh_tasks: Arc<dyn Fn() + Send + Sync>,
    pub toggle_task: Arc<dyn Fn(String) + Send + Sync>,
    pub insert_workspace_file_ref: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    pub chat_stream_shell: ComposerStreamShell,
    pub chat_wires: ChatComposerWires,
    pub stream_busy_memos: ChatStreamBusyMemos,
}

/// 按固定顺序注册全部壳级 `wire_*` 并返回闭包 / 流式壳等产物。
pub(super) fn run_shell_wiring_in_order(app: &AppSignals) -> ShellWiringOutput {
    wire_phase1_chat_session_lifecycle(app);
    wire_phase2_persisted_prefs_dom_and_settings_hooks(app);
    wire_phase3_escape_layered_dismiss(app);

    let refresh_workspace = make_refresh_workspace_for_shell(app);
    let wiring_tail =
        wire_phase4_workspace_status_and_chat_domain(app, Arc::clone(&refresh_workspace));

    ShellWiringOutput {
        refresh_workspace,
        refresh_status: wiring_tail.refresh_status,
        refresh_tasks: wiring_tail.refresh_tasks,
        toggle_task: wiring_tail.toggle_task,
        insert_workspace_file_ref: wiring_tail.insert_workspace_file_ref,
        chat_stream_shell: wiring_tail.chat_stream_shell,
        chat_wires: wiring_tail.chat_wires,
        stream_busy_memos: wiring_tail.stream_busy_memos,
    }
}

/// 阶段 1：会话列表 / 活动会话 / 水合与本地草稿等（必须在其它壳级 `Effect` 之前注册）。
fn wire_phase1_chat_session_lifecycle(app: &AppSignals) {
    wire_chat_session_lifecycle_effects(WireChatSessionLifecycleEffectsArgs::from_app_signals(app));
}

/// 阶段 2：偏好写入 `localStorage`、与 `document` 同步、设置弹窗 LLM 草稿打开时填充。
fn wire_phase2_persisted_prefs_dom_and_settings_hooks(app: &AppSignals) {
    wire_persist_side_panel_view_flags(app.shell_ui.side_panel_view);
    wire_persist_status_bar_visible(app.shell_ui.status_bar_visible);
    wire_persist_agent_role(app.llm_settings.selected_agent_role);
    wire_persist_side_width(app.shell_ui.side_width);
    wire_persist_sidebar_rail_collapsed(app.sidebar.sidebar_rail_collapsed);
    wire_persist_editor_layout_mode(app.shell_ui.editor_layout_mode);
    wire_collapse_sidebar_rail_when_ide_layout(
        app.shell_ui.editor_layout_mode,
        app.sidebar.sidebar_rail_collapsed,
    );
    wire_close_shell_chrome_when_ide_layout(
        app.shell_ui.editor_layout_mode,
        app.sidebar.mobile_nav_open,
        app.chat_composer.chat_find_panel_open,
    );
    wire_persist_ide_editor_prefs(app.ide_editor);
    wire_approval_expanded_follows_pending(
        app.approval.pending_approval,
        app.approval.last_approval_sid,
        app.approval.approval_expanded,
    );
    wire_sync_theme_to_storage_and_dom(app.shell_ui.theme);
    wire_sync_locale_html_lang(app.shell_ui.locale);
    wire_sync_bg_decor_to_storage_and_dom(app.shell_ui.bg_decor);
    wire_sync_ide_layout_dom_and_tauri_chrome(app.shell_ui.editor_layout_mode);
    wire_settings_modal_llm_drafts_on_open(WireSettingsModalLlmDraftsSignals {
        settings_modal: app.modal.settings_modal,
        settings_page: app.modal.settings_page,
        status_tasks: app.to_status_tasks(),
        llm: app.llm_settings,
    });
}

/// 阶段 3：全局 Escape 关闭顺序（依赖弹窗/菜单已存在对应信号）。
fn wire_phase3_escape_layered_dismiss(app: &AppSignals) {
    let shell_escape = ShellEscapeSignals {
        session_context_menu: app.sidebar.session_context_menu,
        sidebar_rail_ctx_menu: app.sidebar.sidebar_rail_ctx_menu,
        chat_find_panel_open: app.chat_composer.chat_find_panel_open,
        sidebar_search_panel_open: app.sidebar.sidebar_search_panel_open,
        view_menu_open: app.shell_ui.view_menu_open,
        ide_menubar_dropdown_open: app.shell_ui.ide_menubar_dropdown_open,
        mobile_nav_open: app.sidebar.mobile_nav_open,
        changelist_modal_open: app.modal.changelist_modal_open,
        settings_modal: app.modal.settings_modal,
        ide_settings_page: app.modal.ide_settings_page,
        session_modal: app.modal.session_modal,
    };
    wire_escape_key_layered_dismiss(shell_escape);
    wire_session_delete_hotkey(SessionDeleteHotkeySignals {
        chat: app.chat,
        draft: app.chat_composer.draft,
        locale: app.shell_ui.locale,
        session_modal: app.modal.session_modal,
        settings_modal: app.modal.settings_modal,
        changelist_modal_open: app.modal.changelist_modal_open,
    });
}

/// 阶段 4a：工作区 / 变更集（须在 [`make_refresh_workspace_for_shell`] 之后）。
fn wire_phase4a_workspace_domain(app: &AppSignals, refresh_workspace: &RefreshWorkspaceFn) {
    wire_workspace_domain_effects(WireWorkspaceDomainEffectsArgs {
        session_sync: app.chat.session_sync,
        changelist_fetch_nonce: app.modal.changelist_fetch_nonce,
        changelist_modal_body: app.modal.changelist_modal_body,
        markdown_render: app.shell_ui.markdown_render,
        changelist_body_ref: app.modal.changelist_body_ref,
        side_panel_view: app.shell_ui.side_panel_view,
        initialized: app.initialized,
        refresh_workspace: Arc::clone(refresh_workspace),
    });
}

/// 阶段 4b：`/status`、`/tasks` 与侧栏任务面可见时的 `Effect`（须在 4a 之后，以便与初始化顺序一致）。
fn wire_phase4b_status_tasks_domain(app: &AppSignals) -> StatusTasksSpawn {
    let refresh_status = make_refresh_status(
        app.to_status_tasks(),
        app.llm_settings.selected_agent_role,
        app.shell_ui.locale.get_untracked(),
    );
    let refresh_tasks =
        make_refresh_tasks(app.to_status_tasks(), app.shell_ui.locale.get_untracked());
    let toggle_task = make_toggle_task(app.to_status_tasks(), app.shell_ui.locale.get_untracked());

    wire_status_tasks_domain_effects(
        app.initialized,
        app.to_status_tasks(),
        Arc::clone(&refresh_status),
        app.shell_ui.side_panel_view,
        Arc::clone(&refresh_tasks),
    );

    StatusTasksSpawn {
        refresh_status,
        refresh_tasks,
        toggle_task,
    }
}

/// 阶段 4c：工作区路径插入 composer、流式壳、聊天列 `wire_*`（与 4a 共用 `refresh_workspace`）。
fn wire_phase4c_chat_and_workspace_chrome(
    app: &AppSignals,
    refresh_workspace: &RefreshWorkspaceFn,
) -> ChatColumnWiringPack {
    let insert_workspace_file_ref: InsertWorkspacePathFn = make_insert_workspace_path_into_composer(
        app.chat_composer.draft,
        app.stream.status_err,
        app.shell_ui.locale,
        app.chat_composer.composer_input_ref.clone(),
    );
    let insert_workspace_file_ref_sv = StoredValue::new(Arc::clone(&insert_workspace_file_ref));

    let chat_stream_shell =
        ComposerStreamShell::from_app_signals(app, Arc::clone(refresh_workspace));

    let (chat_wires, stream_busy_memos) = wire_chat_domain_effects(
        WireChatDomainEffectsArgs::from_app_and_stream_shell(app, chat_stream_shell.clone()),
    );

    crate::session_workspace_partition::wire_workspace_session_storage_partition(
        crate::session_workspace_partition::WireWorkspaceSessionPartitionArgs {
            initialized: app.initialized,
            workspace_data: app.workspace.workspace_data,
            chat: app.chat,
            draft: app.chat_composer.draft,
            locale: app.shell_ui.locale,
            session_sessions_storage_key: app.session_sessions_storage_key,
        },
    );

    crate::session_workspace_bind::wire_session_bound_workspace_effects(
        app.initialized,
        app.chat,
        app.to_workspace_panel(),
        app.shell_ui.locale,
    );

    ChatColumnWiringPack {
        insert_workspace_file_ref: insert_workspace_file_ref_sv,
        chat_stream_shell,
        chat_wires,
        stream_busy_memos,
    }
}

/// 阶段 4：依次调用 4a → 4b → 4c（须在 [`make_refresh_workspace_for_shell`] 之后）。
fn wire_phase4_workspace_status_and_chat_domain(
    app: &AppSignals,
    refresh_workspace: RefreshWorkspaceFn,
) -> Phase4WiringTail {
    wire_phase4a_workspace_domain(app, &refresh_workspace);
    let StatusTasksSpawn {
        refresh_status,
        refresh_tasks,
        toggle_task,
    } = wire_phase4b_status_tasks_domain(app);
    let ChatColumnWiringPack {
        insert_workspace_file_ref,
        chat_stream_shell,
        chat_wires,
        stream_busy_memos,
    } = wire_phase4c_chat_and_workspace_chrome(app, &refresh_workspace);

    Phase4WiringTail {
        refresh_status,
        refresh_tasks,
        toggle_task,
        insert_workspace_file_ref,
        chat_stream_shell,
        chat_wires,
        stream_busy_memos,
    }
}

/// 构建工作区刷新闭包（阶段 3 之后、阶段 4 之前）。
fn make_refresh_workspace_for_shell(app: &AppSignals) -> RefreshWorkspaceFn {
    make_refresh_workspace(
        app.to_workspace_panel(),
        app.shell_ui.locale.get_untracked(),
    )
}
