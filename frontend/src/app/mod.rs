//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好接线）。
//!
//! 首启会话加载、`localStorage` / DOM 偏好同步、全局 `Escape` 等壳级副作用见 `app_shell_effects`。聊天主路径（滚动、查找、输入/流式）见 `chat` 子模块；Workspace / 变更集、`/status` 与任务等接线已部分迁至 **`wire_workspace_domain`**、**`status_tasks_wiring`**、**`chat::wire_chat_session_lifecycle`**（阶段 B）。

mod app_bootstrap_phase;
mod app_shell_bootstrap;
mod app_shell_ctx;
mod app_shell_effects;
mod app_shell_init;
mod app_shell_wire_phases;
mod app_signals;
mod approval_modal;
mod changelist_modal;
mod chat;
mod ide_editor_pane;
mod ide_layout;
mod ide_menu_bar;
mod ide_settings_page;
mod ide_tabs_bar;
pub(crate) mod local_storage_index;
mod mobile_shell_header;
pub mod scroll_guard;
mod session_list_modal;
mod settings_commit;
mod settings_form_state;
mod settings_modal;
mod settings_modal_dialog;
mod settings_models_registry;
mod settings_page;
mod settings_sections;
pub(crate) mod shell_prefs_storage;
mod shell_runtime_context;
mod side_column;
mod side_column_toolbar;
mod side_column_workspace_scroll;
mod sidebar_nav;
mod status_bar;
mod status_tasks_state;
mod status_tasks_wiring;
mod stream_run_phase;
mod stream_shell_busy;
mod tauri_chat_titlebar;
mod tauri_window_controls;
mod wire_workspace_domain;
mod workspace_panel;
pub(crate) mod workspace_panel_state;

use app_shell_init::init_app_shell;
use approval_modal::ApprovalModal;
use changelist_modal::changelist_modal_view;
use chat::ChatFindBar;
use chat::chat_column_view;
use ide_layout::{IdeLayoutShellSignals, IdeLayoutView};
use ide_settings_page::IdeSettingsPageView;
use mobile_shell_header::mobile_shell_header_view;
use session_list_modal::session_list_modal_view;
use settings_modal::settings_modal_view;
use settings_page::SettingsPageView;
use shell_runtime_context::ChatShellLeptosContext;
use side_column::side_column_view;
use sidebar_nav::sidebar_nav_view;
use status_bar::status_bar_footer_view;
use tauri_chat_titlebar::TauriChatTitlebar;

use crate::i18n;

use leptos::prelude::*;

#[component]
pub fn App() -> impl IntoView {
    let app_ctx = init_app_shell();
    provide_context(ChatShellLeptosContext::from_app_signals(&app_ctx.signals));

    // `AppShellCtx` 含 `Rc` 等，不满足 `Send`；子组件闭包不得捕获整 ctx（见 Leptos `ToChildren` 约束）。
    let approval_modal_signals = app_ctx.approval_modal_signals();
    let settings_page_view_input = app_ctx.settings_page_view_input();
    let ide_settings_page_view_input = app_ctx.ide_settings_page_view_input();
    let mobile_shell_header_signals = app_ctx.mobile_shell_header_signals();
    let changelist_modal_signals = app_ctx.changelist_modal_signals();
    let settings_modal_signals = app_ctx.settings_modal_signals();
    let session_list_modal_signals = app_ctx.session_list_modal_signals();
    let status_bar_footer_signals = app_ctx.status_bar_footer_signals();
    let sidebar_nav_signals = app_ctx.sidebar_nav_signals();
    let side_column_view_signals = app_ctx.side_column_view_signals();

    view! {
        <div
            class="app-root app-shell-ds"
            class:sidebar-rail-collapsed=move || app_ctx.signals.sidebar.sidebar_rail_collapsed.get()
            class:app-root--ide-layout=move || app_ctx.signals.shell_ui.editor_layout_mode.get()
        >
            <TauriChatTitlebar
                locale=app_ctx.signals.shell_ui.locale
                editor_layout_mode=app_ctx.signals.shell_ui.editor_layout_mode
            />
            {sidebar_nav_view(sidebar_nav_signals)}

            <Show when=move || app_ctx.signals.sidebar.sidebar_rail_collapsed.get()>
                <button
                    type="button"
                    class="btn btn-secondary sidebar-rail-reveal-btn"
                    prop:aria-label=move || i18n::nav_sidebar_expand_aria(app_ctx.signals.shell_ui.locale.get())
                    on:click=move |_| app_ctx.signals.sidebar.sidebar_rail_collapsed.set(false)
                >
                    "›"
                </button>
            </Show>

            <div
                class="shell-main"
                class:settings-page-hidden=move || {
                    app_ctx.signals.modal.settings_page.get()
                        || app_ctx.signals.modal.ide_settings_page.get()
                }
                class:shell-main--ide-layout=move || app_ctx.signals.shell_ui.editor_layout_mode.get()
            >
                {mobile_shell_header_view(mobile_shell_header_signals)}

                <Show when=move || app_ctx.signals.chat_composer.chat_find_panel_open.get()>
                    <ChatFindBar />
                </Show>

                <div
                    class:main-row-resizing=move || app_ctx.signals.resize.side_resize_dragging.get()
                    class="main-row"
                >
                    <div
                        class="main-row-chat-layer"
                        class:main-row-chat-layer--hidden=move || {
                            app_ctx.signals.shell_ui.editor_layout_mode.get()
                        }
                    >
                        {chat_column_view(app_ctx.chat_column.clone())}
                        {side_column_view(side_column_view_signals.clone())}
                    </div>
                    <div
                        class="main-row-ide-layer"
                        class:main-row-ide-layer--hidden=move || {
                            !app_ctx.signals.shell_ui.editor_layout_mode.get()
                        }
                    >
                        <IdeLayoutView shell=IdeLayoutShellSignals {
                            locale: app_ctx.signals.shell_ui.locale,
                            editor: app_ctx.signals.ide_editor,
                            editor_layout_mode: app_ctx.signals.shell_ui.editor_layout_mode,
                            ide_settings_page: app_ctx.signals.modal.ide_settings_page,
                            ide_menubar_dropdown_open: app_ctx.signals.shell_ui.ide_menubar_dropdown_open,
                            chat: app_ctx.signals.chat,
                            workspace_panel: app_ctx.signals.to_workspace_panel(),
                            refresh_workspace: app_ctx.refresh_workspace.clone(),
                            initialized: app_ctx.signals.initialized,
                            editor_visible: app_ctx.signals.shell_ui.editor_layout_mode,
                        } />
                    </div>
                </div>

                {status_bar_footer_view(status_bar_footer_signals.clone())}
            </div>

            {session_list_modal_view(session_list_modal_signals)}

            {settings_modal_view(settings_modal_signals)}

            {changelist_modal_view(changelist_modal_signals)}

            <ApprovalModal signals=approval_modal_signals />

            <SettingsPageView input=settings_page_view_input />
            <IdeSettingsPageView input=ide_settings_page_view_input />
        </div>
    }
}
