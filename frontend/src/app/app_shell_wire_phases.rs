//! App 壳层 `wire_*` 的**分阶段**注册：编号即 [`super::app_shell_init::init_app_shell`] 中的调用顺序，
//! 重排前须通读各阶段注释并跑前端 wasm 检查。
//!
//! | 阶段 | 职责 |
//! |------|------|
//! | 1 | 会话生命周期、首启与水合相关 `Effect`（须最先）。 |
//! | 2 | 本机偏好持久化、审批条展开跟随、主题/语言/bg DOM、设置打开时 LLM 草稿。 |
//! | 3 | `Escape` 分层关闭（依赖阶段 2 已挂接的弹层/菜单信号）。 |
//! | 4a | 工作区 / 变更集域（依赖 `refresh_workspace`）。 |
//! | 4b | `/status`、`/tasks` 与侧栏任务面可见时的 `Effect`。 |
//! | 4c | 工作区路径插入 composer、流式壳、聊天列 `wire_*`（与 4a 共用 `refresh_workspace`）。 |

use std::sync::Arc;

use leptos::prelude::*;

use super::app_shell_effects::{
    ShellEscapeSignals, WireSettingsModalLlmDraftsSignals, wire_approval_expanded_follows_pending,
    wire_escape_key_layered_dismiss, wire_persist_agent_role, wire_persist_side_panel_view_flags,
    wire_persist_side_width, wire_persist_sidebar_rail_collapsed, wire_persist_status_bar_visible,
    wire_settings_modal_llm_drafts_on_open, wire_sync_bg_decor_to_storage_and_dom,
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

/// 阶段 1：会话列表 / 活动会话 / 水合与本地草稿等（必须在其它壳级 `Effect` 之前注册）。
pub(super) fn wire_phase1_chat_session_lifecycle(app: &AppSignals) {
    wire_chat_session_lifecycle_effects(WireChatSessionLifecycleEffectsArgs {
        initialized: app.initialized,
        sessions: app.chat.sessions,
        active_id: app.chat.active_id,
        draft: app.chat_composer.draft,
        locale: app.shell_ui.locale,
        web_ui_config_loaded: app.shell_ui.web_ui_config_loaded,
        markdown_render: app.shell_ui.markdown_render,
        apply_assistant_display_filters: app.shell_ui.apply_assistant_display_filters,
        chat_session: app.chat,
        selected_agent_role: app.llm_settings.selected_agent_role,
    });
}

/// 阶段 2：偏好写入 `localStorage`、与 `document` 同步、设置弹窗 LLM 草稿打开时填充。
pub(super) fn wire_phase2_persisted_prefs_dom_and_settings_hooks(app: &AppSignals) {
    wire_persist_side_panel_view_flags(app.shell_ui.side_panel_view);
    wire_persist_status_bar_visible(app.shell_ui.status_bar_visible);
    wire_persist_agent_role(app.llm_settings.selected_agent_role);
    wire_persist_side_width(app.shell_ui.side_width);
    wire_persist_sidebar_rail_collapsed(app.sidebar.sidebar_rail_collapsed);
    wire_approval_expanded_follows_pending(
        app.approval.pending_approval,
        app.approval.last_approval_sid,
        app.approval.approval_expanded,
    );
    wire_sync_theme_to_storage_and_dom(app.shell_ui.theme);
    wire_sync_locale_html_lang(app.shell_ui.locale);
    wire_sync_bg_decor_to_storage_and_dom(app.shell_ui.bg_decor);
    wire_settings_modal_llm_drafts_on_open(WireSettingsModalLlmDraftsSignals {
        settings_modal: app.modal.settings_modal,
        settings_page: app.modal.settings_page,
        status_tasks: app.to_status_tasks(),
        llm: app.llm_settings,
    });
}

/// 阶段 3：全局 Escape 关闭顺序（依赖弹窗/菜单已存在对应信号）。
pub(super) fn wire_phase3_escape_layered_dismiss(app: &AppSignals) {
    let shell_escape = ShellEscapeSignals {
        session_context_menu: app.sidebar.session_context_menu,
        sidebar_rail_ctx_menu: app.sidebar.sidebar_rail_ctx_menu,
        chat_find_panel_open: app.chat_composer.chat_find_panel_open,
        sidebar_search_panel_open: app.sidebar.sidebar_search_panel_open,
        view_menu_open: app.shell_ui.view_menu_open,
        mobile_nav_open: app.sidebar.mobile_nav_open,
        changelist_modal_open: app.modal.changelist_modal_open,
        settings_modal: app.modal.settings_modal,
        session_modal: app.modal.session_modal,
    };
    wire_escape_key_layered_dismiss(shell_escape);
}

/// 阶段 4a：工作区 / 变更集（须在 [`make_refresh_workspace_for_shell`] 之后）。
pub(super) fn wire_phase4a_workspace_domain(
    app: &AppSignals,
    refresh_workspace: &Arc<dyn Fn() + Send + Sync>,
) {
    wire_workspace_domain_effects(WireWorkspaceDomainEffectsArgs {
        session_sync: app.chat.session_sync,
        changelist_fetch_nonce: app.modal.changelist_fetch_nonce,
        changelist_modal_loading: app.modal.changelist_modal_loading,
        changelist_modal_err: app.modal.changelist_modal_err,
        changelist_modal_html: app.modal.changelist_modal_html,
        changelist_modal_rev: app.modal.changelist_modal_rev,
        markdown_render: app.shell_ui.markdown_render,
        changelist_body_ref: app.modal.changelist_body_ref,
        side_panel_view: app.shell_ui.side_panel_view,
        initialized: app.initialized,
        refresh_workspace: Arc::clone(refresh_workspace),
    });
}

/// 阶段 4b：`/status`、`/tasks` 与侧栏任务面可见时的 `Effect`（须在 4a 之后，以便与初始化顺序一致）。
pub(super) fn wire_phase4b_status_tasks_domain(
    app: &AppSignals,
) -> (
    Arc<dyn Fn() + Send + Sync>,
    Arc<dyn Fn() + Send + Sync>,
    Arc<dyn Fn(String) + Send + Sync>,
) {
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

    (refresh_status, refresh_tasks, toggle_task)
}

/// 阶段 4c：工作区路径插入 composer、流式壳、聊天列 `wire_*`（与 4a 共用 `refresh_workspace`）。
pub(super) fn wire_phase4c_chat_and_workspace_chrome(
    app: &AppSignals,
    refresh_workspace: &Arc<dyn Fn() + Send + Sync>,
) -> (
    StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    ComposerStreamShell,
    ChatComposerWires,
    ChatStreamBusyMemos,
) {
    let insert_workspace_file_ref: Arc<dyn Fn(String) + Send + Sync> =
        make_insert_workspace_path_into_composer(
            app.chat_composer.draft,
            app.stream.status_err,
            app.shell_ui.locale,
            app.chat_composer.composer_input_ref.clone(),
        );
    let insert_workspace_file_ref_sv = StoredValue::new(Arc::clone(&insert_workspace_file_ref));

    let chat_stream_shell =
        ComposerStreamShell::from_app_signals(app, Arc::clone(refresh_workspace));

    let (chat_wires, stream_busy_memos) = wire_chat_domain_effects(WireChatDomainEffectsArgs {
        initialized: app.initialized,
        chat_session: app.chat,
        draft: app.chat_composer.draft,
        pending_images: app.chat_composer.pending_images,
        pending_clarification: app.approval.pending_clarification,
        collapsed_long_assistant_ids: app.chat_composer.collapsed_long_assistant_ids,
        composer_mirror_html: app.chat_composer.composer_mirror_html,
        composer_mirror_scroll_top: app.chat_composer.composer_mirror_scroll_top,
        composer_input_ref: app.chat_composer.composer_input_ref.clone(),
        sessions: app.chat.sessions,
        active_id: app.chat.active_id,
        messages_scroller: app.chat_composer.messages_scroller,
        auto_scroll_chat: app.chat_composer.auto_scroll_chat,
        messages_scroll_from_effect: app.chat_composer.messages_scroll_from_effect,
        chat_find_query: app.chat_composer.chat_find_query,
        chat_find_match_ids: app.chat_composer.chat_find_match_ids,
        chat_find_cursor: app.chat_composer.chat_find_cursor,
        locale: app.shell_ui.locale,
        apply_assistant_display_filters: app.shell_ui.apply_assistant_display_filters,
        focus_message_id_after_nav: app.chat_composer.focus_message_id_after_nav,
        selected_agent_role: app.llm_settings.selected_agent_role,
        stream_shell: chat_stream_shell.clone(),
    });

    (
        insert_workspace_file_ref_sv,
        chat_stream_shell,
        chat_wires,
        stream_busy_memos,
    )
}

/// 阶段 4：依次调用 4a → 4b → 4c（须在 [`make_refresh_workspace_for_shell`] 之后）。
pub(super) fn wire_phase4_workspace_status_and_chat_domain(
    app: &AppSignals,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
) -> (
    Arc<dyn Fn() + Send + Sync>,
    Arc<dyn Fn() + Send + Sync>,
    Arc<dyn Fn(String) + Send + Sync>,
    StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    ComposerStreamShell,
    ChatComposerWires,
    ChatStreamBusyMemos,
) {
    wire_phase4a_workspace_domain(app, &refresh_workspace);
    let (refresh_status, refresh_tasks, toggle_task) = wire_phase4b_status_tasks_domain(app);
    let (insert_workspace_file_ref_sv, chat_stream_shell, chat_wires, stream_busy_memos) =
        wire_phase4c_chat_and_workspace_chrome(app, &refresh_workspace);

    (
        refresh_status,
        refresh_tasks,
        toggle_task,
        insert_workspace_file_ref_sv,
        chat_stream_shell,
        chat_wires,
        stream_busy_memos,
    )
}

/// 构建工作区刷新闭包（阶段 3 之后、阶段 4 之前）。
pub(super) fn make_refresh_workspace_for_shell(app: &AppSignals) -> Arc<dyn Fn() + Send + Sync> {
    make_refresh_workspace(
        app.to_workspace_panel(),
        app.shell_ui.locale.get_untracked(),
    )
}
