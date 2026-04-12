//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好接线）。
//!
//! 首启会话加载、`localStorage` / DOM 偏好同步、全局 `Escape` 等壳级副作用见 `app_shell_effects`。聊天主路径（滚动、查找、输入/流式）见 `chat` 子模块；Workspace 刷新、`/status` 与任务拉取、变更集等见 `workspace_panel`、`workspace_panel_state`、`status_tasks_wiring`、`changelist_modal`。

mod app_shell_ctx;
mod app_shell_effects;
mod approval_bar;
mod changelist_modal;
mod chat;
mod mobile_shell_header;
pub mod scroll_guard;
mod session_hydrate;
mod session_list_modal;
mod settings_modal;
mod side_column;
mod sidebar_nav;
mod status_bar;
mod status_tasks_state;
mod status_tasks_wiring;
mod workspace_panel;
mod workspace_panel_state;

use app_shell_ctx::AppShellCtx;
use approval_bar::ApprovalBar;
use changelist_modal::{
    changelist_modal_view, wire_changelist_body_inner_html, wire_changelist_fetch_effects,
};
use chat::{
    ChatColumnShell, ChatFindBar, ComposerStreamShell, chat_column_view,
    load_timeline_panel_expanded_default, wire_chat_domain::wire_chat_domain_effects,
};
use mobile_shell_header::mobile_shell_header_view;
use session_list_modal::session_list_modal_view;
use settings_modal::settings_modal_view;
use side_column::side_column_view;
use sidebar_nav::sidebar_nav_view;
use status_bar::status_bar_footer_view;
use status_tasks_state::StatusTasksSignals;
use status_tasks_wiring::{
    make_refresh_status, make_refresh_tasks, make_toggle_task,
    wire_status_fetch_if_missing_after_init, wire_tasks_refresh_when_tasks_panel_visible,
};
use workspace_panel::{
    make_insert_workspace_path_into_composer, make_refresh_workspace,
    wire_workspace_refresh_when_visible,
};
use workspace_panel_state::WorkspacePanelSignals;

use app_shell_effects::{
    ShellEscapeSignals, wire_approval_expanded_follows_pending, wire_context_used_estimate,
    wire_escape_key_layered_dismiss, wire_initial_sessions_from_storage, wire_persist_agent_role,
    wire_persist_chat_sessions, wire_persist_side_panel_view_flags, wire_persist_side_width,
    wire_persist_sidebar_rail_collapsed, wire_persist_status_bar_visible,
    wire_settings_modal_llm_drafts_on_open, wire_sync_bg_decor_to_storage_and_dom,
    wire_sync_locale_html_lang, wire_sync_theme_to_storage_and_dom,
    wire_web_ui_config_once_after_init,
};

use crate::chat_session_state::ChatSessionSignals;
use session_hydrate::wire_session_hydration;

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::api::{StatusData, TasksData, WorkspaceData};
use crate::app_prefs::{
    AGENT_ROLE_KEY, BG_DECOR_KEY, DEFAULT_SIDE_WIDTH, SIDEBAR_RAIL_COLLAPSED_KEY,
    STATUS_BAR_VISIBLE_KEY, THEME_KEY, WORKSPACE_WIDTH_KEY, load_bool_key, load_f64_key,
    load_side_panel_view, local_storage,
};
use crate::clarification_form::PendingClarificationForm;
use crate::i18n::{self, load_locale_from_storage};
use crate::session_ops::SessionContextAnchor;
use crate::session_sync::SessionSyncState;
use crate::storage::ChatSession;

use leptos::html::{Div, Textarea};
use leptos::prelude::*;
use leptos_dom::helpers::WindowListenerHandle;

#[component]
pub fn App() -> impl IntoView {
    let sessions = RwSignal::new(Vec::<ChatSession>::new());
    let active_id = RwSignal::new(String::new());
    let initialized = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let pending_images = RwSignal::new(Vec::<String>::new());
    let pending_clarification = RwSignal::new(None::<PendingClarificationForm>);
    // 输入草稿：仅写 Mutex，不在每键 `sessions.update`；发送 / 切会话时再写入 `ChatSession.draft`。
    let composer_draft_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let composer_input_ref: NodeRef<Textarea> = NodeRef::new();
    // 本地会话与后端 `conversation_id` / `revision` 的单一聚合状态（见 `session_sync.rs`）。
    let session_sync = RwSignal::new(SessionSyncState::local_only());
    // 递增后触发：从 `GET /conversation/messages` 水合当前会话（与 `server_conversation_id` 对齐）。
    let session_hydrate_nonce = RwSignal::new(0_u64);
    // 当前 `/chat/stream` 任务 `job_id`（响应头与 `sse_capabilities`）；断线重连用。
    let stream_job_id = RwSignal::new(None::<u64>);
    // 已消费的最大 SSE `id:`；与 `stream_resume.after_seq` / `Last-Event-ID` 对齐。
    let stream_last_event_seq = RwSignal::new(0u64);
    let chat_session = ChatSessionSignals {
        sessions,
        active_id,
        session_sync,
        session_hydrate_nonce,
        stream_job_id,
        stream_last_event_seq,
    };
    // 超长已完成助手消息默认全文展示；在此列表中的 id 表示用户手动折叠。
    let collapsed_long_assistant_ids = RwSignal::new(Vec::<String>::new());
    // 连续工具输出分组：以组内首条消息 id 为键，表示该组处于展开态（默认折叠只显示最新一条）。
    let expanded_tool_run_heads = RwSignal::new(HashSet::<String>::new());
    let side_panel_view = RwSignal::new(load_side_panel_view());
    let view_menu_open = RwSignal::new(false);
    let status_bar_visible = RwSignal::new(load_bool_key(STATUS_BAR_VISIBLE_KEY, true));
    let side_width = RwSignal::new(load_f64_key(WORKSPACE_WIDTH_KEY, DEFAULT_SIDE_WIDTH));
    let theme = RwSignal::new(
        local_storage()
            .and_then(|s| s.get_item(THEME_KEY).ok().flatten())
            .unwrap_or_else(|| "dark".to_string()),
    );
    let bg_decor = RwSignal::new(load_bool_key(BG_DECOR_KEY, true));
    let status_busy = RwSignal::new(false);
    let status_err = RwSignal::new(None::<String>);
    let tool_busy = RwSignal::new(false);
    let workspace_data = RwSignal::new(None::<WorkspaceData>);
    let workspace_subtree_expanded = RwSignal::new(HashSet::<String>::new());
    let workspace_subtree_cache = RwSignal::new(HashMap::<String, WorkspaceData>::new());
    let workspace_subtree_loading = RwSignal::new(HashSet::<String>::new());
    let workspace_err = RwSignal::new(None::<String>);
    let workspace_loading = RwSignal::new(false);
    let workspace_path_draft = RwSignal::new(String::new());
    let workspace_set_err = RwSignal::new(None::<String>);
    let workspace_set_busy = RwSignal::new(false);
    let workspace_pick_busy = RwSignal::new(false);
    let workspace_panel = WorkspacePanelSignals {
        workspace_data,
        workspace_subtree_expanded,
        workspace_subtree_cache,
        workspace_subtree_loading,
        workspace_err,
        workspace_loading,
        workspace_path_draft,
        workspace_set_err,
        workspace_set_busy,
        workspace_pick_busy,
    };
    let status_data = RwSignal::new(None::<StatusData>);
    let status_loading = RwSignal::new(true);
    // `GET /status` 失败时的说明（与流式对话错误 `status_err` 区分）。
    let status_fetch_err = RwSignal::new(None::<String>);
    let selected_agent_role = RwSignal::new(
        local_storage()
            .and_then(|s| s.get_item(AGENT_ROLE_KEY).ok().flatten())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
    );
    let tasks_data = RwSignal::new(TasksData { items: vec![] });
    let tasks_err = RwSignal::new(None::<String>);
    let tasks_loading = RwSignal::new(false);
    let status_tasks = StatusTasksSignals {
        status_data,
        status_loading,
        status_fetch_err,
        tasks_data,
        tasks_err,
        tasks_loading,
    };
    let pending_approval = RwSignal::new(None::<(String, String, String)>);
    let session_modal = RwSignal::new(false);
    let settings_modal = RwSignal::new(false);
    let llm_api_base_draft = RwSignal::new(String::new());
    let llm_api_base_preset_select = RwSignal::new(String::from("server"));
    let llm_model_draft = RwSignal::new(String::new());
    let llm_api_key_draft = RwSignal::new(String::new());
    let llm_has_saved_key = RwSignal::new(false);
    let llm_settings_feedback = RwSignal::new(None::<String>);
    // 本机模型设置写入后递增，使状态栏订阅并重新读取 localStorage。
    let client_llm_storage_tick = RwSignal::new(0_u64);
    let session_context_menu = RwSignal::new(None::<SessionContextAnchor>);
    let mobile_nav_open = RwSignal::new(false);
    // 桌面端左侧会话栏是否收起（`localStorage` `SIDEBAR_RAIL_COLLAPSED_KEY`）。
    let sidebar_rail_collapsed = RwSignal::new(load_bool_key(SIDEBAR_RAIL_COLLAPSED_KEY, false));
    let approval_expanded = RwSignal::new(false);
    let last_approval_sid = RwSignal::new(String::new());
    let abort_cell: Arc<Mutex<Option<web_sys::AbortController>>> = Arc::new(Mutex::new(None));
    // 用户点「停止」后为 true，避免异步 on_done / on_error 覆盖已写入的「已停止」文案。
    let user_cancelled_stream: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));
    let messages_scroller = NodeRef::<Div>::new();
    // 为 false 时表示用户已离开底部，流式输出不再强行跟底；滚回底部附近会重新置 true。
    let auto_scroll_chat = RwSignal::new(true);
    // Effect 程序化滚底时置 true，避免 `scroll_height` 已变而 `scrollTop` 尚未跟上时，`on:scroll` 误判 gap 并关掉跟底。
    let messages_scroll_from_effect = RwSignal::new(false);
    // 记录滚动方向：仅当用户向下回到底部附近时才恢复自动跟底，避免上滚初期抖动。
    let last_messages_scroll_top = RwSignal::new(0_i32);
    // 侧栏：按标题过滤会话。
    let sidebar_session_query = RwSignal::new(String::new());
    // 侧栏：跨会话消息全文搜索（本地）。
    let global_message_query = RwSignal::new(String::new());
    // 侧栏：筛选/跨会话搜索输入区默认收起，由会话列表空白处右键菜单打开。
    let sidebar_search_panel_open = RwSignal::new(false);
    let sidebar_rail_ctx_menu = RwSignal::new(None::<(f64, f64)>);
    // 主区：当前会话内查找。
    let chat_find_query = RwSignal::new(String::new());
    let chat_find_match_ids = RwSignal::new(Vec::<String>::new());
    let chat_find_cursor = RwSignal::new(0_usize);
    let chat_find_panel_open = RwSignal::new(false);
    let timeline_panel_expanded = RwSignal::new(load_timeline_panel_expanded_default());
    // 从侧栏跳转后滚动到该消息（DOM 就绪后消费）。
    let focus_message_id_after_nav = RwSignal::new(None::<String>);
    let changelist_modal_open = RwSignal::new(false);
    let changelist_modal_loading = RwSignal::new(false);
    let changelist_modal_err = RwSignal::new(None::<String>);
    let changelist_modal_html = RwSignal::new(String::new());
    let changelist_modal_rev = RwSignal::new(0_u64);
    let changelist_body_ref = NodeRef::<Div>::new();
    // 递增后由 Effect 拉取 GET /workspace/changelog（避免在 view 中捕获非 Send 的 Rc<dyn Fn>）。
    let changelist_fetch_nonce = RwSignal::new(0_u64);
    let locale = RwSignal::new(load_locale_from_storage());
    // 当前会话消息 + 草稿字符数（本地估算），对照 `/status.context_char_budget`。
    let context_used_estimate = RwSignal::new(0_usize);
    // 与 GET /web-ui、环境变量 AGENT_WEB_DISABLE_MARKDOWN 对齐；拉取失败时保持 true（沿用 Markdown）。
    let markdown_render = RwSignal::new(true);
    // 与 AGENT_WEB_RAW_ASSISTANT_OUTPUT 对齐；为 false 时助手展示/搜索/导出均不过滤原文。
    let apply_assistant_display_filters = RwSignal::new(true);
    let web_ui_config_loaded = RwSignal::new(false);

    wire_initial_sessions_from_storage(initialized, sessions, active_id, draft, locale);
    wire_web_ui_config_once_after_init(
        initialized,
        web_ui_config_loaded,
        markdown_render,
        apply_assistant_display_filters,
    );

    wire_session_hydration(
        initialized,
        web_ui_config_loaded,
        chat_session,
        locale,
        selected_agent_role,
    );

    wire_persist_chat_sessions(initialized, sessions, active_id);
    wire_context_used_estimate(
        initialized,
        sessions,
        active_id,
        draft,
        context_used_estimate,
    );
    wire_persist_side_panel_view_flags(side_panel_view);
    wire_persist_status_bar_visible(status_bar_visible);
    wire_persist_agent_role(selected_agent_role);
    wire_persist_side_width(side_width);
    wire_persist_sidebar_rail_collapsed(sidebar_rail_collapsed);
    wire_approval_expanded_follows_pending(pending_approval, last_approval_sid, approval_expanded);
    wire_sync_theme_to_storage_and_dom(theme);
    wire_sync_locale_html_lang(locale);
    wire_sync_bg_decor_to_storage_and_dom(bg_decor);
    wire_settings_modal_llm_drafts_on_open(
        settings_modal,
        status_tasks,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        llm_settings_feedback,
    );
    let shell_escape = ShellEscapeSignals {
        session_context_menu,
        sidebar_rail_ctx_menu,
        chat_find_panel_open,
        sidebar_search_panel_open,
        view_menu_open,
        mobile_nav_open,
        changelist_modal_open,
        settings_modal,
        session_modal,
    };
    wire_escape_key_layered_dismiss(shell_escape);

    let refresh_workspace = make_refresh_workspace(workspace_panel);

    wire_changelist_fetch_effects(
        session_sync,
        changelist_fetch_nonce,
        changelist_modal_loading,
        changelist_modal_err,
        changelist_modal_html,
        changelist_modal_rev,
        markdown_render,
    );
    wire_changelist_body_inner_html(changelist_modal_html, changelist_body_ref);

    wire_workspace_refresh_when_visible(
        side_panel_view,
        initialized,
        Arc::clone(&refresh_workspace),
    );

    let refresh_status = make_refresh_status(status_tasks, selected_agent_role);
    let refresh_tasks = make_refresh_tasks(status_tasks);
    let toggle_task = make_toggle_task(status_tasks);

    wire_status_fetch_if_missing_after_init(initialized, status_tasks, Arc::clone(&refresh_status));
    wire_tasks_refresh_when_tasks_panel_visible(
        side_panel_view,
        initialized,
        Arc::clone(&refresh_tasks),
    );

    let insert_workspace_file_ref: Arc<dyn Fn(String) + Send + Sync> =
        make_insert_workspace_path_into_composer(
            Arc::clone(&composer_draft_buffer),
            draft,
            status_err,
            locale,
            composer_input_ref.clone(),
        );
    let insert_workspace_file_ref_sv = StoredValue::new(Arc::clone(&insert_workspace_file_ref));

    let thinking_trace_log = RwSignal::new(Vec::<crate::sse_dispatch::ThinkingTraceInfo>::new());
    let chat_stream_shell = ComposerStreamShell {
        status_busy,
        status_err,
        pending_approval,
        tool_busy,
        abort_cell: Arc::clone(&abort_cell),
        user_cancelled_stream: Arc::clone(&user_cancelled_stream),
        refresh_workspace: Arc::clone(&refresh_workspace),
        changelist_modal_open,
        changelist_fetch_nonce,
        pending_clarification,
        thinking_trace_log,
    };

    let chat_wires = wire_chat_domain_effects(
        initialized,
        chat_session,
        draft,
        pending_images,
        pending_clarification,
        collapsed_long_assistant_ids,
        Arc::clone(&composer_draft_buffer),
        composer_input_ref.clone(),
        sessions,
        active_id,
        messages_scroller,
        auto_scroll_chat,
        messages_scroll_from_effect,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        locale,
        apply_assistant_display_filters,
        focus_message_id_after_nav,
        selected_agent_role,
        chat_stream_shell.clone(),
    );

    let side_resize_session: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));
    let side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>> =
        Rc::new(RefCell::new(None));
    let side_resize_dragging = RwSignal::new(false);

    let new_session = Rc::clone(&chat_wires.new_session);

    let app_ctx = AppShellCtx {
        locale,
        mobile_nav_open,
        session_modal,
        new_session,
        sidebar_session_query,
        global_message_query,
        sidebar_search_panel_open,
        sidebar_rail_ctx_menu,
        chat_find_panel_open,
        chat: chat_session,
        draft,
        focus_message_id_after_nav,
        session_context_menu,
        composer_draft_buffer: Arc::clone(&composer_draft_buffer),
        apply_assistant_display_filters,
        sidebar_rail_collapsed,
        side_resize_dragging,
        side_panel_view,
        side_width,
        side_resize_session: Rc::clone(&side_resize_session),
        side_resize_handles: Rc::clone(&side_resize_handles),
        view_menu_open,
        status_bar_visible,
        settings_modal,
        workspace_panel,
        status_tasks,
        refresh_workspace: Arc::clone(&refresh_workspace),
        refresh_tasks: Arc::clone(&refresh_tasks),
        toggle_task: Arc::clone(&toggle_task),
        changelist_modal_open,
        changelist_fetch_nonce,
        insert_workspace_file_ref: insert_workspace_file_ref_sv,
        thinking_trace_log: chat_stream_shell.thinking_trace_log,
        status_err,
        tool_busy,
        status_busy,
        client_llm_storage_tick,
        selected_agent_role,
        context_used_estimate,
        refresh_status: Arc::clone(&refresh_status),
        theme,
        bg_decor,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        llm_settings_feedback,
        changelist_modal_loading,
        changelist_modal_err,
        changelist_modal_rev,
        changelist_body_ref,
        chat_column: ChatColumnShell {
            locale,
            messages_scroller,
            auto_scroll_chat,
            messages_scroll_from_effect,
            last_messages_scroll_top,
            timeline_panel_expanded,
            chat: chat_session,
            collapsed_long_assistant_ids,
            expanded_tool_run_heads,
            chat_find_query,
            chat_find_match_ids,
            chat_find_cursor,
            composer_input_ref,
            composer_buf_ta: Arc::clone(&composer_draft_buffer),
            pending_images,
            stream_shell: chat_stream_shell.clone(),
            run_send_message: chat_wires.run_send_message.clone(),
            trigger_stop: Arc::clone(&chat_wires.cancel_stream),
            initialized,
            regen_stream_after_truncate: chat_wires.regen_stream_after_truncate,
            retry_assistant_target: chat_wires.retry_assistant_target,
            markdown_render,
            apply_assistant_display_filters,
        },
    };

    view! {
        <div
            class="app-root app-shell-ds"
            class:sidebar-rail-collapsed=move || sidebar_rail_collapsed.get()
        >
            {sidebar_nav_view(app_ctx.clone())}

            <Show when=move || sidebar_rail_collapsed.get()>
                <button
                    type="button"
                    class="btn btn-secondary sidebar-rail-reveal-btn"
                    prop:aria-label=move || i18n::nav_sidebar_expand_aria(locale.get())
                    on:click=move |_| sidebar_rail_collapsed.set(false)
                >
                    "›"
                </button>
            </Show>

            <div class="shell-main">
                {mobile_shell_header_view(app_ctx.clone())}

                <ApprovalBar
                    pending_approval=pending_approval
                    approval_expanded=approval_expanded
                    locale=locale
                />

                <Show when=move || chat_find_panel_open.get()>
                    <ChatFindBar
                        chat_find_panel_open=chat_find_panel_open
                        locale=locale
                        chat_find_query=chat_find_query
                        chat_find_match_ids=chat_find_match_ids
                        chat_find_cursor=chat_find_cursor
                        auto_scroll_chat=auto_scroll_chat
                    />
                </Show>

                <div
                    class:main-row-resizing=move || side_resize_dragging.get()
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
        </div>
    }
}
