//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好副作用）。
//!
//! 聊天滚动、查找、输入/流式、Workspace 刷新、变更集拉取等副作用拆至子模块，见 `chat_scroll`、`chat_find`、`chat_composer`、`workspace_panel`、`changelist_modal`。

mod approval_bar;
mod changelist_modal;
mod chat_column;
mod chat_composer;
mod chat_export_menu;
mod chat_find;
mod chat_find_bar;
mod chat_message_render;
mod chat_scroll;
mod mobile_shell_header;
pub mod scroll_guard;
mod session_list_modal;
mod settings_modal;
mod side_column;
mod sidebar_nav;
mod status_bar;
mod timeline_panel;
mod workspace_panel;

use approval_bar::ApprovalBar;
use changelist_modal::{
    changelist_modal_view, wire_changelist_body_inner_html, wire_changelist_fetch_effects,
};
use chat_column::chat_column_view;
use chat_composer::{
    wire_chat_composer_streams, wire_draft_sync_to_buffer_and_textarea,
    wire_session_switch_clears_chat_state,
};
use chat_export_menu::ChatExportContextMenu;
use chat_find::wire_chat_find_matches;
use chat_find_bar::ChatFindBar;
use chat_scroll::{wire_focus_message_after_nav, wire_messages_auto_scroll};
use mobile_shell_header::mobile_shell_header_view;
use session_list_modal::session_list_modal_view;
use settings_modal::settings_modal_view;
use side_column::side_column_view;
use sidebar_nav::sidebar_nav_view;
use status_bar::status_bar_footer_view;
use timeline_panel::load_timeline_panel_expanded_default;
use workspace_panel::{make_refresh_workspace, wire_workspace_refresh_when_visible};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::api::{
    StatusData, TasksData, WorkspaceData, client_llm_storage_has_api_key,
    fetch_conversation_messages, fetch_status, fetch_tasks, fetch_web_ui_config,
    load_client_llm_text_fields_from_storage, save_tasks,
};
use crate::app_prefs::{
    AGENT_ROLE_KEY, BG_DECOR_KEY, DEFAULT_SIDE_WIDTH, STATUS_BAR_VISIBLE_KEY, SidePanelView,
    TASKS_VISIBLE_KEY, THEME_KEY, WORKSPACE_VISIBLE_KEY, WORKSPACE_WIDTH_KEY, load_bool_key,
    load_f64_key, load_side_panel_view, local_storage, store_bool_key, store_f64_key,
    store_side_panel_view,
};
use crate::clarification_form::PendingClarificationForm;
use crate::conversation_hydrate::stored_messages_from_conversation_api;
use crate::i18n::{self, load_locale_from_storage};
use crate::session_ops::{
    SessionContextAnchor, estimate_context_chars_for_active_session, title_from_user_prompt,
};
use crate::session_sync::SessionSyncState;
use crate::storage::{ChatSession, ensure_at_least_one, load_sessions, save_sessions};

use gloo_timers::future::TimeoutFuture;
use leptos::html::{Div, Textarea};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::{WindowListenerHandle, window_event_listener};
use wasm_bindgen::JsCast;

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
    // 已完成长助手消息默认折叠；在此列表中的 id 表示已展开。
    let expanded_long_assistant_ids = RwSignal::new(Vec::<String>::new());
    // 连续工具输出分组：以组内首条消息 id 为键，表示该组处于展开态（默认折叠只显示最新一条）。
    let expanded_tool_run_heads = RwSignal::new(HashSet::<String>::new());
    // 连续分阶段时间线旁注分组：键同工具分组。
    let expanded_staged_timeline_heads = RwSignal::new(HashSet::<String>::new());
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
    // 主区：多选聊天气泡导出 Markdown（由聊天区右键菜单进入）。
    let bubble_md_select_mode = RwSignal::new(false);
    let bubble_md_selected_ids = RwSignal::new(Vec::<String>::new());
    let chat_export_ctx_menu = RwSignal::new(None::<(f64, f64, Option<String>)>);
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

    Effect::new(move |_| {
        if initialized.get() {
            return;
        }
        let (list, aid) = load_sessions();
        let (list, def_id) = ensure_at_least_one(
            list,
            i18n::default_session_title(locale.get_untracked()).to_string(),
        );
        let pick = aid
            .filter(|id| list.iter().any(|s| s.id == *id))
            .unwrap_or(def_id);
        let d = list
            .iter()
            .find(|s| s.id == pick)
            .map(|s| s.draft.clone())
            .unwrap_or_default();
        sessions.set(list);
        active_id.set(pick);
        draft.set(d);
        initialized.set(true);
    });

    Effect::new({
        let markdown_render = markdown_render;
        let apply_assistant_display_filters = apply_assistant_display_filters;
        move |_| {
            if !initialized.get() || web_ui_config_loaded.get() {
                return;
            }
            web_ui_config_loaded.set(true);
            spawn_local(async move {
                if let Ok(c) = fetch_web_ui_config().await {
                    markdown_render.set(c.markdown_render);
                    apply_assistant_display_filters.set(c.apply_assistant_display_filters);
                }
            });
        }
    });

    Effect::new({
        let sessions = sessions;
        let active_id = active_id;
        let locale = locale;
        let initialized = initialized;
        let web_ui_config_loaded = web_ui_config_loaded;
        let selected_agent_role = selected_agent_role;
        let session_sync = session_sync;
        let session_hydrate_nonce = session_hydrate_nonce;
        move |_| {
            if !initialized.get() || !web_ui_config_loaded.get() {
                return;
            }
            let _ = session_hydrate_nonce.get();
            let aid = active_id.get();
            if aid.is_empty() {
                return;
            }
            let Some(cid) = sessions.with(|list| {
                list.iter().find(|s| s.id == aid).and_then(|s| {
                    // 如果当前会话有正在流式更新的消息，跳过水合，避免覆盖本地新消息
                    if s.messages
                        .iter()
                        .any(|m| m.state.as_deref() == Some("loading"))
                    {
                        return None;
                    }
                    let c = s
                        .server_conversation_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|x| !x.is_empty())?;
                    Some(c.to_string())
                })
            }) else {
                return;
            };
            let loc = locale.get_untracked();
            spawn_local(async move {
                let Ok(resp) = fetch_conversation_messages(&cid, loc).await else {
                    return;
                };
                let msgs = stored_messages_from_conversation_api(&resp.messages);
                sessions.update(|list| {
                    if active_id.get_untracked() != aid {
                        return;
                    }
                    let Some(s) = list.iter_mut().find(|x| x.id == aid) else {
                        return;
                    };
                    // 再次检查：异步拉取期间可能已开始新的流式更新
                    if s.messages
                        .iter()
                        .any(|m| m.state.as_deref() == Some("loading"))
                    {
                        return;
                    }
                    let still = s
                        .server_conversation_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|x| !x.is_empty());
                    if still != Some(cid.as_str()) {
                        return;
                    }
                    s.messages = msgs;
                    s.server_revision = Some(resp.revision);
                    if let Some(role) = resp
                        .active_agent_role
                        .as_deref()
                        .map(str::trim)
                        .filter(|r| !r.is_empty())
                    {
                        selected_agent_role.set(Some(role.to_string()));
                    }
                    let user_count = s.messages.iter().filter(|m| m.role == "user").count();
                    if user_count == 1 && i18n::is_default_session_title(&s.title) {
                        if let Some(u) = s.messages.iter().find(|m| m.role == "user") {
                            s.title = title_from_user_prompt(&u.text);
                        }
                    }
                });
                session_sync.update(|st| {
                    if st.conversation_id.as_deref().map(str::trim) == Some(cid.as_str()) {
                        st.apply_saved_revision(resp.revision);
                    }
                });
            });
        }
    });

    Effect::new(move |_| {
        if !initialized.get() {
            return;
        }
        let list = sessions.get();
        let aid = active_id.get();
        if aid.is_empty() {
            return;
        }
        save_sessions(&list, Some(&aid));
    });

    Effect::new({
        let sessions = sessions;
        let active_id = active_id;
        let draft = draft;
        move |_| {
            if !initialized.get() {
                return;
            }
            let n = estimate_context_chars_for_active_session(
                &sessions.get(),
                active_id.get().as_str(),
                draft.get().as_str(),
            );
            context_used_estimate.set(n);
        }
    });

    Effect::new(move |_| {
        let v = side_panel_view.get();
        store_side_panel_view(v);
        store_bool_key(WORKSPACE_VISIBLE_KEY, matches!(v, SidePanelView::Workspace));
        store_bool_key(TASKS_VISIBLE_KEY, matches!(v, SidePanelView::Tasks));
    });
    Effect::new(move |_| {
        store_bool_key(STATUS_BAR_VISIBLE_KEY, status_bar_visible.get());
    });
    Effect::new(move |_| {
        if let Some(st) = local_storage() {
            match selected_agent_role
                .get()
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                Some(role) => {
                    let _ = st.set_item(AGENT_ROLE_KEY, role);
                }
                None => {
                    let _ = st.remove_item(AGENT_ROLE_KEY);
                }
            }
        }
    });
    Effect::new(move |_| {
        store_f64_key(WORKSPACE_WIDTH_KEY, side_width.get());
    });

    Effect::new(move |_| {
        if let Some((sid, _, _)) = pending_approval.get() {
            if last_approval_sid.get_untracked() != sid {
                last_approval_sid.set(sid);
                approval_expanded.set(false);
            }
        } else {
            last_approval_sid.set(String::new());
        }
    });

    Effect::new(move |_| {
        let t = theme.get();
        if let Some(st) = local_storage() {
            let _ = st.set_item(THEME_KEY, &t);
        }
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            let _ = root.set_attribute("data-theme", &t);
        }
    });

    Effect::new(move |_| {
        let lang = locale.get().html_lang();
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            let _ = doc
                .document_element()
                .map(|root| root.set_attribute("lang", lang));
        }
    });

    Effect::new(move |_| {
        store_bool_key(BG_DECOR_KEY, bg_decor.get());
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            if bg_decor.get() {
                let _ = root.remove_attribute("data-bg-decor");
            } else {
                let _ = root.set_attribute("data-bg-decor", "plain");
            }
        }
    });

    Effect::new(move |_| {
        if !settings_modal.get() {
            return;
        }
        let (stored_base, stored_model) = load_client_llm_text_fields_from_storage();
        let sd = status_data.get_untracked();
        let base = if stored_base.trim().is_empty() {
            sd.as_ref().map(|d| d.api_base.clone()).unwrap_or_default()
        } else {
            stored_base
        };
        let model = if stored_model.trim().is_empty() {
            sd.as_ref().map(|d| d.model.clone()).unwrap_or_default()
        } else {
            stored_model
        };
        llm_api_base_draft.set(base.clone());
        llm_api_base_preset_select.set(
            crate::client_llm_presets::api_base_select_value_for_draft(base.as_str()).to_string(),
        );
        llm_model_draft.set(model);
        llm_api_key_draft.set(String::new());
        llm_has_saved_key.set(client_llm_storage_has_api_key());
        llm_settings_feedback.set(None);
    });

    let refresh_workspace = make_refresh_workspace(
        workspace_loading,
        workspace_err,
        workspace_path_draft,
        workspace_data,
        workspace_subtree_expanded,
        workspace_subtree_cache,
        workspace_subtree_loading,
    );

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

    let refresh_tasks: Arc<dyn Fn() + Send + Sync> = Arc::new({
        move || {
            tasks_loading.set(true);
            spawn_local(async move {
                match fetch_tasks().await {
                    Ok(d) => {
                        tasks_err.set(None);
                        tasks_data.set(d);
                    }
                    Err(e) => {
                        tasks_err.set(Some(e));
                    }
                }
                tasks_loading.set(false);
            });
        }
    });

    let refresh_status: Arc<dyn Fn() + Send + Sync> = Arc::new({
        move || {
            status_loading.set(true);
            status_fetch_err.set(None);
            spawn_local(async move {
                match fetch_status().await {
                    Ok(d) => {
                        status_fetch_err.set(None);
                        if let Some(cur) = selected_agent_role.get_untracked()
                            && !d.agent_role_ids.iter().any(|id| id == &cur)
                        {
                            selected_agent_role.set(None);
                        }
                        status_data.set(Some(d));
                    }
                    Err(e) => {
                        status_data.set(None);
                        status_fetch_err.set(Some(e));
                    }
                }
                status_loading.set(false);
            });
        }
    });

    Effect::new({
        let refresh_status = Arc::clone(&refresh_status);
        move |_| {
            if initialized.get() && status_data.get().is_none() {
                refresh_status();
            }
        }
    });

    Effect::new({
        let refresh_tasks = Arc::clone(&refresh_tasks);
        move |_| {
            if matches!(side_panel_view.get(), SidePanelView::Tasks) && initialized.get() {
                refresh_tasks();
            }
        }
    });

    wire_session_switch_clears_chat_state(
        initialized,
        sessions,
        active_id,
        draft,
        pending_images,
        pending_clarification,
        session_sync,
        stream_job_id,
        stream_last_event_seq,
        expanded_long_assistant_ids,
        bubble_md_selected_ids,
    );

    wire_draft_sync_to_buffer_and_textarea(
        draft,
        Arc::clone(&composer_draft_buffer),
        composer_input_ref.clone(),
    );

    wire_messages_auto_scroll(
        sessions,
        active_id,
        messages_scroller,
        auto_scroll_chat,
        messages_scroll_from_effect,
    );

    wire_chat_find_matches(
        sessions,
        active_id,
        chat_find_query,
        chat_find_match_ids,
        chat_find_cursor,
        auto_scroll_chat,
        locale,
        apply_assistant_display_filters,
    );

    wire_focus_message_after_nav(focus_message_id_after_nav);

    let insert_workspace_file_ref: Arc<dyn Fn(String) + Send + Sync> = Arc::new({
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        let draft = draft;
        let status_err = status_err;
        let locale = locale;
        let composer_input_ref = composer_input_ref.clone();
        move |rel: String| {
            if rel.chars().any(|c| c.is_whitespace()) {
                status_err.set(Some(
                    i18n::composer_ws_path_whitespace_err(locale.get_untracked()).to_string(),
                ));
                return;
            }
            let token = format!("@{rel}");
            let mut guard = composer_draft_buffer.lock().unwrap();
            let needs_space = guard
                .chars()
                .next_back()
                .is_some_and(|c| !c.is_whitespace());
            if needs_space {
                guard.push(' ');
            }
            guard.push_str(&token);
            guard.push(' ');
            let next = guard.clone();
            drop(guard);
            draft.set(next.clone());
            status_err.set(None);
            let cref = composer_input_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = cref.get() {
                    let _ = el.focus();
                }
            });
        }
    });
    let insert_workspace_file_ref_sv = StoredValue::new(Arc::clone(&insert_workspace_file_ref));

    let chat_wires = wire_chat_composer_streams(
        initialized,
        sessions,
        locale,
        active_id,
        draft,
        session_hydrate_nonce,
        session_sync,
        stream_job_id,
        stream_last_event_seq,
        selected_agent_role,
        status_busy,
        status_err,
        pending_approval,
        tool_busy,
        Arc::clone(&composer_draft_buffer),
        auto_scroll_chat,
        Arc::clone(&abort_cell),
        Arc::clone(&user_cancelled_stream),
        Arc::clone(&refresh_workspace),
        changelist_modal_open,
        changelist_fetch_nonce,
        pending_images,
        pending_clarification,
    );

    let toggle_task: Arc<dyn Fn(String) + Send + Sync> = Arc::new({
        move |id: String| {
            let mut next = tasks_data.get();
            if let Some(i) = next.items.iter().position(|t| t.id == id) {
                next.items[i].done = !next.items[i].done;
                let n = next.clone();
                spawn_local(async move {
                    if let Ok(saved) = save_tasks(&n).await {
                        tasks_data.set(saved);
                    }
                });
            }
        }
    });

    let side_resize_session: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));
    let side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>> =
        Rc::new(RefCell::new(None));
    let side_resize_dragging = RwSignal::new(false);

    let composer_buf_nav = Arc::clone(&composer_draft_buffer);
    let composer_buf_ta = Arc::clone(&composer_draft_buffer);

    let new_session = {
        let inner = chat_wires.new_session.clone();
        move || inner()
    };

    Effect::new(move |_| {
        let h = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
            if ev.key() != "Escape" {
                return;
            }
            if let Some(t) = ev.target() {
                if let Ok(he) = t.dyn_into::<web_sys::HtmlElement>() {
                    let tag = he.tag_name();
                    if tag.eq_ignore_ascii_case("TEXTAREA")
                        || tag.eq_ignore_ascii_case("INPUT")
                        || tag.eq_ignore_ascii_case("SELECT")
                        || tag.eq_ignore_ascii_case("OPTION")
                    {
                        return;
                    }
                    if he.is_content_editable() {
                        return;
                    }
                }
            }
            ev.prevent_default();
            if chat_export_ctx_menu.get_untracked().is_some() {
                chat_export_ctx_menu.set(None);
                return;
            }
            if session_context_menu.get_untracked().is_some() {
                session_context_menu.set(None);
                return;
            }
            if sidebar_rail_ctx_menu.get_untracked().is_some() {
                sidebar_rail_ctx_menu.set(None);
                return;
            }
            if chat_find_panel_open.get_untracked() {
                chat_find_panel_open.set(false);
                return;
            }
            if sidebar_search_panel_open.get_untracked() {
                sidebar_search_panel_open.set(false);
                return;
            }
            if view_menu_open.get_untracked() {
                view_menu_open.set(false);
                return;
            }
            if mobile_nav_open.get_untracked() {
                mobile_nav_open.set(false);
                return;
            }
            if changelist_modal_open.get_untracked() {
                changelist_modal_open.set(false);
                return;
            }
            if settings_modal.get_untracked() {
                settings_modal.set(false);
                return;
            }
            if session_modal.get_untracked() {
                session_modal.set(false);
            }
        });
        on_cleanup(move || h.remove());
    });

    view! {
        <div class="app-root app-shell-ds">
            {sidebar_nav_view(
                locale,
                mobile_nav_open,
                session_modal,
                new_session.clone(),
                sidebar_session_query,
                global_message_query,
                sidebar_search_panel_open,
                sidebar_rail_ctx_menu,
                chat_find_panel_open,
                sessions,
                active_id,
                draft,
                session_sync,
                focus_message_id_after_nav,
                session_context_menu,
                composer_buf_nav.clone(),
                apply_assistant_display_filters,
            )}

            <div class="shell-main">
                {mobile_shell_header_view(mobile_nav_open, locale, new_session.clone())}

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

                <Show when=move || chat_export_ctx_menu.get().is_some()>
                    <ChatExportContextMenu
                        chat_export_ctx_menu=chat_export_ctx_menu
                        locale=locale
                        bubble_md_select_mode=bubble_md_select_mode
                        bubble_md_selected_ids=bubble_md_selected_ids
                        sessions=sessions
                        active_id=active_id
                        apply_assistant_display_filters=apply_assistant_display_filters
                    />
                </Show>

                <div
                    class:main-row-resizing=move || side_resize_dragging.get()
                    class="main-row"
                >
                    {chat_column_view(
                        locale,
                        messages_scroller,
                        auto_scroll_chat,
                        messages_scroll_from_effect,
                        last_messages_scroll_top,
                        session_context_menu,
                        chat_export_ctx_menu,
                        chat_find_panel_open,
                        timeline_panel_expanded,
                        bubble_md_select_mode,
                        bubble_md_selected_ids,
                        sessions,
                        active_id,
                        expanded_long_assistant_ids,
                        expanded_tool_run_heads,
                        expanded_staged_timeline_heads,
                        chat_find_query,
                        chat_find_match_ids,
                        chat_find_cursor,
                        composer_input_ref,
                        composer_buf_ta.clone(),
                        pending_images,
                        pending_clarification,
                        chat_wires.run_send_message.clone(),
                        Arc::clone(&chat_wires.cancel_stream),
                        status_busy,
                        initialized,
                        session_sync,
                        chat_wires.regen_stream_after_truncate,
                        chat_wires.retry_assistant_target,
                        status_err,
                        markdown_render,
                        apply_assistant_display_filters,
                    )}

                    {side_column_view(
                        locale,
                        side_resize_dragging,
                        side_panel_view,
                        side_width,
                        side_resize_session.clone(),
                        side_resize_handles.clone(),
                        view_menu_open,
                        status_bar_visible,
                        settings_modal,
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
                        tasks_data,
                        tasks_err,
                        tasks_loading,
                        Arc::clone(&refresh_workspace),
                        Arc::clone(&refresh_tasks),
                        Arc::clone(&toggle_task),
                        changelist_modal_open,
                        changelist_fetch_nonce,
                        insert_workspace_file_ref_sv,
                    )}
                </div>

                {status_bar_footer_view(
                    status_bar_visible,
                    status_fetch_err,
                    status_err,
                    tool_busy,
                    status_busy,
                    status_loading,
                    status_data,
                    client_llm_storage_tick,
                    selected_agent_role,
                    stream_job_id,
                    stream_last_event_seq,
                    session_sync,
                    context_used_estimate,
                    Arc::clone(&refresh_status),
                    locale,
                )}
            </div>

            {session_list_modal_view(
                session_modal,
                locale,
                sessions,
                active_id,
                draft,
                session_sync,
                composer_draft_buffer.clone(),
                apply_assistant_display_filters,
            )}

            {settings_modal_view(
                settings_modal,
                locale,
                theme,
                bg_decor,
                status_data,
                llm_api_base_draft,
                llm_api_base_preset_select,
                llm_model_draft,
                llm_api_key_draft,
                llm_has_saved_key,
                llm_settings_feedback,
                client_llm_storage_tick,
            )}

            {changelist_modal_view(
                changelist_modal_open,
                locale,
                changelist_modal_loading,
                changelist_modal_err,
                changelist_modal_rev,
                changelist_fetch_nonce,
                changelist_body_ref,
            )}
        </div>
    }
}
