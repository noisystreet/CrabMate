//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好接线）。
//!
//! 首启会话加载、`localStorage` / DOM 偏好同步、全局 `Escape` 等壳级副作用见 `app_shell_effects`。聊天主路径（滚动、查找、输入/流式）见 `chat` 子模块；Workspace / 变更集、`/status` 与任务等接线已部分迁至 **`wire_workspace_domain`**、**`status_tasks_wiring`**、**`chat::wire_chat_session_lifecycle`**（阶段 B）。

mod app_shell_ctx;
mod app_shell_effects;
mod app_signals;
mod approval_modal;
mod changelist_modal;
mod chat;
mod mobile_shell_header;
pub mod scroll_guard;
mod session_hydrate;
mod session_list_modal;
mod settings_modal;
mod settings_page;
mod side_column;
mod sidebar_nav;
mod status_bar;
mod status_tasks_state;
mod status_tasks_wiring;
mod wire_workspace_domain;
mod workspace_panel;
mod workspace_panel_state;

use app_shell_ctx::AppShellCtx;
use app_signals::AppSignals;
use approval_modal::ApprovalModal;
use changelist_modal::changelist_modal_view;
use chat::{
    ChatColumnShell, ChatFindBar, ComposerStreamShell, chat_column_view,
    wire_chat_domain::wire_chat_domain_effects,
    wire_chat_session_lifecycle::wire_chat_session_lifecycle_effects,
};
use mobile_shell_header::mobile_shell_header_view;
use session_list_modal::session_list_modal_view;
use settings_modal::settings_modal_view;
use settings_page::SettingsPageView;
use side_column::side_column_view;
use sidebar_nav::sidebar_nav_view;
use status_bar::status_bar_footer_view;
use status_tasks_wiring::{
    make_refresh_status, make_refresh_tasks, make_toggle_task, wire_status_tasks_domain_effects,
};
use wire_workspace_domain::wire_workspace_domain_effects;
use workspace_panel::{make_insert_workspace_path_into_composer, make_refresh_workspace};

use app_shell_effects::{
    ShellEscapeSignals, wire_approval_expanded_follows_pending, wire_escape_key_layered_dismiss,
    wire_persist_agent_role, wire_persist_side_panel_view_flags, wire_persist_side_width,
    wire_persist_sidebar_rail_collapsed, wire_persist_status_bar_visible,
    wire_settings_modal_llm_drafts_on_open, wire_sync_bg_decor_to_storage_and_dom,
    wire_sync_locale_html_lang, wire_sync_theme_to_storage_and_dom,
};

use crate::i18n;

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    let app_signals = AppSignals::new();

    wire_chat_session_lifecycle_effects(
        app_signals.initialized,
        app_signals.chat.sessions,
        app_signals.chat.active_id,
        app_signals.chat_composer.draft,
        app_signals.shell_ui.locale,
        app_signals.shell_ui.web_ui_config_loaded,
        app_signals.shell_ui.markdown_render,
        app_signals.shell_ui.apply_assistant_display_filters,
        app_signals.chat,
        app_signals.llm_settings.selected_agent_role,
    );
    wire_persist_side_panel_view_flags(app_signals.shell_ui.side_panel_view);
    wire_persist_status_bar_visible(app_signals.shell_ui.status_bar_visible);
    wire_persist_agent_role(app_signals.llm_settings.selected_agent_role);
    wire_persist_side_width(app_signals.shell_ui.side_width);
    wire_persist_sidebar_rail_collapsed(app_signals.sidebar.sidebar_rail_collapsed);
    wire_approval_expanded_follows_pending(
        app_signals.approval.pending_approval,
        app_signals.approval.last_approval_sid,
        app_signals.approval.approval_expanded,
    );
    wire_sync_theme_to_storage_and_dom(app_signals.shell_ui.theme);
    wire_sync_locale_html_lang(app_signals.shell_ui.locale);
    wire_sync_bg_decor_to_storage_and_dom(app_signals.shell_ui.bg_decor);
    wire_settings_modal_llm_drafts_on_open(
        app_signals.modal.settings_modal,
        app_signals.to_status_tasks(),
        app_signals.llm_settings.llm_api_base_draft,
        app_signals.llm_settings.llm_api_base_preset_select,
        app_signals.llm_settings.llm_model_draft,
        app_signals.llm_settings.llm_api_key_draft,
        app_signals.llm_settings.llm_has_saved_key,
        app_signals.llm_settings.llm_settings_feedback,
        app_signals.llm_settings.executor_llm_api_base_draft,
        app_signals.llm_settings.executor_llm_api_base_preset_select,
        app_signals.llm_settings.executor_llm_model_draft,
        app_signals.llm_settings.executor_llm_api_key_draft,
        app_signals.llm_settings.executor_llm_has_saved_key,
        app_signals.llm_settings.executor_llm_settings_feedback,
    );
    let shell_escape = ShellEscapeSignals {
        session_context_menu: app_signals.sidebar.session_context_menu,
        sidebar_rail_ctx_menu: app_signals.sidebar.sidebar_rail_ctx_menu,
        chat_find_panel_open: app_signals.chat_composer.chat_find_panel_open,
        sidebar_search_panel_open: app_signals.sidebar.sidebar_search_panel_open,
        view_menu_open: app_signals.shell_ui.view_menu_open,
        mobile_nav_open: app_signals.sidebar.mobile_nav_open,
        changelist_modal_open: app_signals.modal.changelist_modal_open,
        settings_modal: app_signals.modal.settings_modal,
        session_modal: app_signals.modal.session_modal,
    };
    wire_escape_key_layered_dismiss(shell_escape);

    let refresh_workspace = make_refresh_workspace(
        app_signals.to_workspace_panel(),
        app_signals.shell_ui.locale.get_untracked(),
    );

    wire_workspace_domain_effects(
        app_signals.chat.session_sync,
        app_signals.modal.changelist_fetch_nonce,
        app_signals.modal.changelist_modal_loading,
        app_signals.modal.changelist_modal_err,
        app_signals.modal.changelist_modal_html,
        app_signals.modal.changelist_modal_rev,
        app_signals.shell_ui.markdown_render,
        app_signals.modal.changelist_body_ref,
        app_signals.shell_ui.side_panel_view,
        app_signals.initialized,
        Arc::clone(&refresh_workspace),
    );

    let refresh_status = make_refresh_status(
        app_signals.to_status_tasks(),
        app_signals.llm_settings.selected_agent_role,
        app_signals.shell_ui.locale.get_untracked(),
    );
    let refresh_tasks = make_refresh_tasks(
        app_signals.to_status_tasks(),
        app_signals.shell_ui.locale.get_untracked(),
    );
    let toggle_task = make_toggle_task(
        app_signals.to_status_tasks(),
        app_signals.shell_ui.locale.get_untracked(),
    );

    wire_status_tasks_domain_effects(
        app_signals.initialized,
        app_signals.to_status_tasks(),
        Arc::clone(&refresh_status),
        app_signals.shell_ui.side_panel_view,
        Arc::clone(&refresh_tasks),
    );

    let insert_workspace_file_ref: Arc<dyn Fn(String) + Send + Sync> =
        make_insert_workspace_path_into_composer(
            Arc::clone(&app_signals.chat_composer.composer_draft_buffer),
            app_signals.chat_composer.draft,
            app_signals.stream.status_err,
            app_signals.shell_ui.locale,
            app_signals.chat_composer.composer_input_ref.clone(),
        );
    let insert_workspace_file_ref_sv = StoredValue::new(Arc::clone(&insert_workspace_file_ref));

    let chat_stream_shell = ComposerStreamShell {
        status_busy: app_signals.stream.status_busy,
        status_err: app_signals.stream.status_err,
        pending_approval: app_signals.approval.pending_approval,
        tool_busy: app_signals.stream.tool_busy,
        abort_cell: Arc::clone(&app_signals.stream.abort_cell),
        user_cancelled_stream: Arc::clone(&app_signals.stream.user_cancelled_stream),
        refresh_workspace: Arc::clone(&refresh_workspace),
        changelist_modal_open: app_signals.modal.changelist_modal_open,
        changelist_fetch_nonce: app_signals.modal.changelist_fetch_nonce,
        pending_clarification: app_signals.approval.pending_clarification,
        thinking_trace_log: app_signals.approval.thinking_trace_log,
    };

    let chat_wires = wire_chat_domain_effects(
        app_signals.initialized,
        app_signals.chat,
        app_signals.chat_composer.draft,
        app_signals.chat_composer.pending_images,
        app_signals.approval.pending_clarification,
        app_signals.chat_composer.collapsed_long_assistant_ids,
        Arc::clone(&app_signals.chat_composer.composer_draft_buffer),
        app_signals.chat_composer.composer_input_ref.clone(),
        app_signals.chat.sessions,
        app_signals.chat.active_id,
        app_signals.chat_composer.messages_scroller,
        app_signals.chat_composer.auto_scroll_chat,
        app_signals.chat_composer.messages_scroll_from_effect,
        app_signals.chat_composer.chat_find_query,
        app_signals.chat_composer.chat_find_match_ids,
        app_signals.chat_composer.chat_find_cursor,
        app_signals.shell_ui.locale,
        app_signals.shell_ui.apply_assistant_display_filters,
        app_signals.chat_composer.focus_message_id_after_nav,
        app_signals.llm_settings.selected_agent_role,
        chat_stream_shell.clone(),
    );

    let new_session = Rc::clone(&chat_wires.new_session);

    let app_ctx = AppShellCtx {
        locale: app_signals.shell_ui.locale,
        mobile_nav_open: app_signals.sidebar.mobile_nav_open,
        session_modal: app_signals.modal.session_modal,
        new_session,
        sidebar_session_query: app_signals.sidebar.sidebar_session_query,
        global_message_query: app_signals.sidebar.global_message_query,
        sidebar_search_panel_open: app_signals.sidebar.sidebar_search_panel_open,
        sidebar_rail_ctx_menu: app_signals.sidebar.sidebar_rail_ctx_menu,
        chat_find_panel_open: app_signals.chat_composer.chat_find_panel_open,
        chat: app_signals.chat,
        draft: app_signals.chat_composer.draft,
        focus_message_id_after_nav: app_signals.chat_composer.focus_message_id_after_nav,
        session_context_menu: app_signals.sidebar.session_context_menu,
        composer_draft_buffer: Arc::clone(&app_signals.chat_composer.composer_draft_buffer),
        apply_assistant_display_filters: app_signals.shell_ui.apply_assistant_display_filters,
        sidebar_rail_collapsed: app_signals.sidebar.sidebar_rail_collapsed,
        side_resize_dragging: app_signals.resize.side_resize_dragging,
        side_panel_view: app_signals.shell_ui.side_panel_view,
        side_width: app_signals.shell_ui.side_width,
        side_resize_session: Rc::clone(&app_signals.resize.side_resize_session),
        side_resize_handles: Rc::clone(&app_signals.resize.side_resize_handles),
        view_menu_open: app_signals.shell_ui.view_menu_open,
        status_bar_visible: app_signals.shell_ui.status_bar_visible,
        settings_modal: app_signals.modal.settings_modal,
        settings_page: app_signals.modal.settings_page,
        workspace_panel: app_signals.to_workspace_panel(),
        status_tasks: app_signals.to_status_tasks(),
        refresh_workspace: Arc::clone(&refresh_workspace),
        refresh_tasks: Arc::clone(&refresh_tasks),
        toggle_task: Arc::clone(&toggle_task),
        changelist_modal_open: app_signals.modal.changelist_modal_open,
        changelist_fetch_nonce: app_signals.modal.changelist_fetch_nonce,
        insert_workspace_file_ref: insert_workspace_file_ref_sv,
        thinking_trace_log: chat_stream_shell.thinking_trace_log,
        status_err: app_signals.stream.status_err,
        tool_busy: app_signals.stream.tool_busy,
        status_busy: app_signals.stream.status_busy,
        client_llm_storage_tick: app_signals.llm_settings.client_llm_storage_tick,
        selected_agent_role: app_signals.llm_settings.selected_agent_role,
        refresh_status: Arc::clone(&refresh_status),
        theme: app_signals.shell_ui.theme,
        bg_decor: app_signals.shell_ui.bg_decor,
        llm_api_base_draft: app_signals.llm_settings.llm_api_base_draft,
        llm_api_base_preset_select: app_signals.llm_settings.llm_api_base_preset_select,
        llm_model_draft: app_signals.llm_settings.llm_model_draft,
        llm_api_key_draft: app_signals.llm_settings.llm_api_key_draft,
        llm_has_saved_key: app_signals.llm_settings.llm_has_saved_key,
        llm_settings_feedback: app_signals.llm_settings.llm_settings_feedback,
        executor_llm_api_base_draft: app_signals.llm_settings.executor_llm_api_base_draft,
        executor_llm_api_base_preset_select: app_signals
            .llm_settings
            .executor_llm_api_base_preset_select,
        executor_llm_model_draft: app_signals.llm_settings.executor_llm_model_draft,
        executor_llm_api_key_draft: app_signals.llm_settings.executor_llm_api_key_draft,
        executor_llm_has_saved_key: app_signals.llm_settings.executor_llm_has_saved_key,
        executor_llm_settings_feedback: app_signals.llm_settings.executor_llm_settings_feedback,
        changelist_modal_loading: app_signals.modal.changelist_modal_loading,
        changelist_modal_err: app_signals.modal.changelist_modal_err,
        changelist_modal_rev: app_signals.modal.changelist_modal_rev,
        changelist_body_ref: app_signals.modal.changelist_body_ref,
        chat_column: ChatColumnShell {
            locale: app_signals.shell_ui.locale,
            messages_scroller: app_signals.chat_composer.messages_scroller,
            auto_scroll_chat: app_signals.chat_composer.auto_scroll_chat,
            messages_scroll_from_effect: app_signals.chat_composer.messages_scroll_from_effect,
            last_messages_scroll_top: app_signals.chat_composer.last_messages_scroll_top,
            timeline_panel_expanded: app_signals.chat_composer.timeline_panel_expanded,
            chat: app_signals.chat,
            collapsed_long_assistant_ids: app_signals.chat_composer.collapsed_long_assistant_ids,
            expanded_tool_run_heads: app_signals.chat_composer.expanded_tool_run_heads,
            chat_find_query: app_signals.chat_composer.chat_find_query,
            chat_find_match_ids: app_signals.chat_composer.chat_find_match_ids,
            chat_find_cursor: app_signals.chat_composer.chat_find_cursor,
            composer_input_ref: app_signals.chat_composer.composer_input_ref,
            composer_buf_ta: Arc::clone(&app_signals.chat_composer.composer_draft_buffer),
            pending_images: app_signals.chat_composer.pending_images,
            stream_shell: chat_stream_shell.clone(),
            run_send_message: chat_wires.run_send_message.clone(),
            trigger_stop: Arc::clone(&chat_wires.cancel_stream),
            initialized: app_signals.initialized,
            regen_stream_after_truncate: chat_wires.regen_stream_after_truncate,
            retry_assistant_target: chat_wires.retry_assistant_target,
            markdown_render: app_signals.shell_ui.markdown_render,
            apply_assistant_display_filters: app_signals.shell_ui.apply_assistant_display_filters,
        },
    };

    view! {
        <div
            class="app-root app-shell-ds"
            class:sidebar-rail-collapsed=move || app_signals.sidebar.sidebar_rail_collapsed.get()
        >
            {sidebar_nav_view(app_ctx.clone())}

            <Show when=move || app_signals.sidebar.sidebar_rail_collapsed.get()>
                <button
                    type="button"
                    class="btn btn-secondary sidebar-rail-reveal-btn"
                    prop:aria-label=move || i18n::nav_sidebar_expand_aria(app_signals.shell_ui.locale.get())
                    on:click=move |_| app_signals.sidebar.sidebar_rail_collapsed.set(false)
                >
                    "›"
                </button>
            </Show>

            <div class="shell-main" class:settings-page-hidden=move || app_signals.modal.settings_page.get()>
                {mobile_shell_header_view(app_ctx.clone())}

                <Show when=move || app_signals.chat_composer.chat_find_panel_open.get()>
                    <ChatFindBar
                        chat_find_panel_open=app_signals.chat_composer.chat_find_panel_open
                        locale=app_signals.shell_ui.locale
                        chat_find_query=app_signals.chat_composer.chat_find_query
                        chat_find_match_ids=app_signals.chat_composer.chat_find_match_ids
                        chat_find_cursor=app_signals.chat_composer.chat_find_cursor
                        auto_scroll_chat=app_signals.chat_composer.auto_scroll_chat
                    />
                </Show>

                <div
                    class:main-row-resizing=move || app_signals.resize.side_resize_dragging.get()
                    class="main-row"
                >
                    {chat_column_view(app_ctx.chat_column.clone())}

                    {side_column_view(app_ctx.clone())}
                </div>

                {status_bar_footer_view(app_ctx.clone())}
            </div>

            {session_list_modal_view(app_ctx.clone())}

            {settings_modal_view(app_ctx.clone())}

            {changelist_modal_view(app_ctx.clone())}

            <ApprovalModal
                pending_approval=app_signals.approval.pending_approval
                locale=app_signals.shell_ui.locale
            />

            <SettingsPageView
                settings_page=app_signals.modal.settings_page
                locale=app_signals.shell_ui.locale
                theme=app_signals.shell_ui.theme
                bg_decor=app_signals.shell_ui.bg_decor
                llm_api_base_draft=app_signals.llm_settings.llm_api_base_draft
                llm_api_base_preset_select=app_signals.llm_settings.llm_api_base_preset_select
                llm_model_draft=app_signals.llm_settings.llm_model_draft
                llm_api_key_draft=app_signals.llm_settings.llm_api_key_draft
                llm_has_saved_key=app_signals.llm_settings.llm_has_saved_key
                llm_settings_feedback=app_signals.llm_settings.llm_settings_feedback
                executor_llm_api_base_draft=app_signals.llm_settings.executor_llm_api_base_draft
                executor_llm_api_base_preset_select=app_signals.llm_settings.executor_llm_api_base_preset_select
                executor_llm_model_draft=app_signals.llm_settings.executor_llm_model_draft
                executor_llm_api_key_draft=app_signals.llm_settings.executor_llm_api_key_draft
                executor_llm_has_saved_key=app_signals.llm_settings.executor_llm_has_saved_key
                executor_llm_settings_feedback=app_signals.llm_settings.executor_llm_settings_feedback
                client_llm_storage_tick=app_signals.llm_settings.client_llm_storage_tick
            />
        </div>
    }
}
