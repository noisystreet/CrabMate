//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好接线）。
//!
//! 首启会话加载、`localStorage` / DOM 偏好同步、全局 `Escape` 等壳级副作用见 `app_shell_effects`。聊天主路径（滚动、查找、输入/流式）见 `chat` 子模块；Workspace / 变更集、`/status` 与任务等接线已部分迁至 **`wire_workspace_domain`**、**`status_tasks_wiring`**、**`chat::wire_chat_session_lifecycle`**（阶段 B）。

mod app_shell_ctx;
mod app_shell_effects;
mod app_shell_init;
mod app_signals;
mod approval_modal;
mod changelist_modal;
mod chat;
mod mobile_shell_header;
pub mod scroll_guard;
mod session_hydrate;
mod session_list_modal;
mod settings_commit;
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

use app_shell_init::{AppShellInit, init_app_shell};
use approval_modal::ApprovalModal;
use changelist_modal::changelist_modal_view;
use chat::ChatFindBar;
use chat::chat_column_view;
use mobile_shell_header::mobile_shell_header_view;
use session_list_modal::session_list_modal_view;
use settings_modal::settings_modal_view;
use settings_page::SettingsPageView;
use side_column::side_column_view;
use sidebar_nav::sidebar_nav_view;
use status_bar::status_bar_footer_view;

use crate::i18n;

use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    let AppShellInit {
        app_signals,
        app_ctx,
    } = init_app_shell();

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
