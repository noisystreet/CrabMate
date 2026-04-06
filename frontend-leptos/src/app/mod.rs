//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好副作用）。

mod approval_bar;
mod changelist_modal;
mod chat_column;
mod chat_export_menu;
mod chat_find_bar;
mod mobile_shell_header;
pub mod scroll_guard;
mod session_list_modal;
mod settings_modal;
mod side_column;
mod sidebar_nav;
mod status_bar;

use approval_bar::ApprovalBar;
use changelist_modal::changelist_modal_view;
use chat_column::chat_column_view;
use chat_export_menu::ChatExportContextMenu;
use chat_find_bar::ChatFindBar;
use mobile_shell_header::mobile_shell_header_view;
use session_list_modal::session_list_modal_view;
use settings_modal::settings_modal_view;
use side_column::side_column_view;
use sidebar_nav::sidebar_nav_view;
use status_bar::status_bar_footer_view;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::api::{
    ChatStreamCallbacks, StatusData, TasksData, WorkspaceData, client_llm_storage_has_api_key,
    fetch_status, fetch_tasks, fetch_workspace_changelog, load_client_llm_text_fields_from_storage,
    save_tasks, send_chat_stream,
};
use crate::app_prefs::{
    AGENT_ROLE_KEY, BG_DECOR_KEY, DEFAULT_SIDE_WIDTH, STATUS_BAR_VISIBLE_KEY, SidePanelView,
    TASKS_VISIBLE_KEY, THEME_KEY, WORKSPACE_VISIBLE_KEY, WORKSPACE_WIDTH_KEY, load_bool_key,
    load_f64_key, load_side_panel_view, local_storage, store_bool_key, store_f64_key,
    store_side_panel_view,
};
use crate::markdown;
use crate::message_format::{message_text_for_display, tool_card_text};
use crate::session_ops::{
    SessionContextAnchor, approval_session_id, flush_composer_draft_to_session, make_message_id,
    message_created_ms, patch_active_session, prepare_retry_failed_assistant_turn,
    title_from_user_prompt,
};
use crate::session_search::{normalize_search_query, scroll_message_into_view};
use crate::sse_dispatch::{CommandApprovalRequest, ToolResultInfo};
use crate::storage::{
    ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, ensure_at_least_one, load_sessions,
    make_session_id, save_sessions,
};
use crate::workspace_shell::reload_workspace_panel;

use gloo_timers::future::TimeoutFuture;
use leptos::html::{Div, Textarea};
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::WindowListenerHandle;
use wasm_bindgen::JsCast;

#[component]
pub fn App() -> impl IntoView {
    let sessions = RwSignal::new(Vec::<ChatSession>::new());
    let active_id = RwSignal::new(String::new());
    let initialized = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    // 输入草稿：仅写 Mutex，不在每键 `sessions.update`；发送 / 切会话时再写入 `ChatSession.draft`。
    let composer_draft_buffer: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let composer_input_ref: NodeRef<Textarea> = NodeRef::new();
    let conversation_id = RwSignal::new(None::<String>);
    // 最近一次 SSE `conversation_saved.revision`；`POST /chat/branch` 需要与服务端一致。
    let conversation_revision = RwSignal::new(None::<u64>);
    // 已完成长助手消息默认折叠；在此列表中的 id 表示已展开。
    let expanded_long_assistant_ids = RwSignal::new(Vec::<String>::new());
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
    // 主区：当前会话内查找。
    let chat_find_query = RwSignal::new(String::new());
    let chat_find_match_ids = RwSignal::new(Vec::<String>::new());
    let chat_find_cursor = RwSignal::new(0_usize);
    let chat_find_panel_open = RwSignal::new(false);
    // 主区：多选聊天气泡导出 Markdown（由聊天区右键菜单进入）。
    let bubble_md_select_mode = RwSignal::new(false);
    let bubble_md_selected_ids = RwSignal::new(Vec::<String>::new());
    let chat_export_ctx_menu = RwSignal::new(None::<(f64, f64)>);
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

    Effect::new(move |_| {
        if initialized.get() {
            return;
        }
        let (list, aid) = load_sessions();
        let (list, def_id) = ensure_at_least_one(list);
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
        llm_api_base_draft.set(base);
        llm_model_draft.set(model);
        llm_api_key_draft.set(String::new());
        llm_has_saved_key.set(client_llm_storage_has_api_key());
        llm_settings_feedback.set(None);
    });

    let refresh_workspace: Arc<dyn Fn() + Send + Sync> = Arc::new({
        move || {
            spawn_local(async move {
                reload_workspace_panel(
                    workspace_loading,
                    workspace_err,
                    workspace_path_draft,
                    workspace_data,
                )
                .await;
            });
        }
    });

    Effect::new({
        let conversation_id = conversation_id;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        let changelist_modal_loading = changelist_modal_loading;
        let changelist_modal_err = changelist_modal_err;
        let changelist_modal_html = changelist_modal_html;
        let changelist_modal_rev = changelist_modal_rev;
        move |_| {
            let n = changelist_fetch_nonce.get();
            if n == 0 {
                return;
            }
            changelist_modal_loading.set(true);
            changelist_modal_err.set(None);
            let cid = conversation_id.get();
            spawn_local(async move {
                match fetch_workspace_changelog(cid.as_deref()).await {
                    Ok(r) => {
                        if let Some(e) = r.error {
                            changelist_modal_err.set(Some(e));
                            changelist_modal_html.set(String::new());
                            changelist_modal_rev.set(0);
                        } else {
                            changelist_modal_rev.set(r.revision);
                            changelist_modal_html.set(markdown::to_safe_html(&r.markdown));
                        }
                    }
                    Err(e) => {
                        changelist_modal_err.set(Some(e));
                        changelist_modal_html.set(String::new());
                        changelist_modal_rev.set(0);
                    }
                }
                changelist_modal_loading.set(false);
            });
        }
    });

    Effect::new({
        let changelist_modal_html = changelist_modal_html;
        let changelist_body_ref = changelist_body_ref.clone();
        move |_| {
            let html = changelist_modal_html.get();
            let r = changelist_body_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(n) = r.get()
                    && let Ok(he) = n.dyn_into::<web_sys::HtmlElement>()
                {
                    he.set_inner_html(&html);
                }
            });
        }
    });

    Effect::new({
        let refresh_workspace = Arc::clone(&refresh_workspace);
        move |_| {
            if matches!(side_panel_view.get(), SidePanelView::Workspace) && initialized.get() {
                refresh_workspace();
            }
        }
    });

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

    // 仅随 `active_id` 切换加载草稿；勿订阅 `sessions`，否则流式更新会反复覆盖输入缓冲。
    Effect::new(move |_| {
        let id = active_id.get();
        if !initialized.get() {
            return;
        }
        let list = sessions.get_untracked();
        let d = list
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.draft.clone())
            .unwrap_or_default();
        draft.set(d);
        conversation_id.set(None);
        conversation_revision.set(None);
        expanded_long_assistant_ids.set(Vec::new());
        bubble_md_selected_ids.set(Vec::new());
    });

    // `draft` 仅程序化更新：同步到 Mutex 与 textarea（输入过程不订阅 `draft`）。
    Effect::new({
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        let composer_input_ref = composer_input_ref.clone();
        move |_| {
            let d = draft.get();
            *composer_draft_buffer.lock().unwrap() = d.clone();
            let d_for_dom = d.clone();
            let cref = composer_input_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = cref.get() {
                    if el.value() != d_for_dom {
                        el.set_value(&d_for_dom);
                    }
                }
            });
        }
    });

    Effect::new(move |_| {
        let aid = active_id.get();
        let _fingerprint = sessions.with(|list| {
            list.iter()
                .find(|s| s.id == aid)
                .map(|s| {
                    s.messages
                        .iter()
                        .fold(0u64, |acc, m| acc.wrapping_add(m.text.len() as u64))
                        .wrapping_add((s.messages.len() as u64).saturating_mul(17))
                })
                .unwrap_or(0)
        });

        if !auto_scroll_chat.get() {
            return;
        }

        let mref = messages_scroller;
        let follow = auto_scroll_chat;
        let scroll_from_effect = messages_scroll_from_effect;
        spawn_local(async move {
            let _scroll_from_effect_guard =
                scroll_guard::MessagesScrollFromEffectGuard::new(scroll_from_effect);
            if !follow.get_untracked() {
                return;
            }
            TimeoutFuture::new(0).await;
            if !follow.get_untracked() {
                return;
            }
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
            }
            TimeoutFuture::new(0).await;
            if !follow.get_untracked() {
                return;
            }
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
            }
            // 再等一帧：流式换行后布局高度可能在本轮 paint 后才稳定
            TimeoutFuture::new(16).await;
            if !follow.get_untracked() {
                return;
            }
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    });

    // 当前会话内查找：匹配列表随消息更新；仅当查询或会话切换时重置光标并滚到首条。
    let chat_find_prev_key: Rc<RefCell<(String, String)>> =
        Rc::new(RefCell::new((String::new(), String::new())));
    Effect::new({
        let sessions = sessions;
        let active_id = active_id;
        let chat_find_query = chat_find_query;
        let chat_find_match_ids = chat_find_match_ids;
        let chat_find_cursor = chat_find_cursor;
        let auto_scroll_chat = auto_scroll_chat;
        let chat_find_prev_key = Rc::clone(&chat_find_prev_key);
        move |_| {
            let aid = active_id.get();
            let q = normalize_search_query(&chat_find_query.get());
            let ids = if q.is_empty() {
                Vec::new()
            } else {
                sessions.with(|list| {
                    list.iter()
                        .find(|s| s.id == aid)
                        .map(|s| {
                            s.messages
                                .iter()
                                .filter(|m| message_text_for_display(m).to_lowercase().contains(&q))
                                .map(|m| m.id.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default()
                })
            };
            chat_find_match_ids.set(ids.clone());
            let mut prev = chat_find_prev_key.borrow_mut();
            let key_changed = prev.0 != q || prev.1 != aid;
            if key_changed {
                prev.0 = q.clone();
                prev.1 = aid.clone();
                chat_find_cursor.set(0);
                if !q.is_empty() && !ids.is_empty() {
                    auto_scroll_chat.set(false);
                    let first = ids[0].clone();
                    spawn_local(async move {
                        TimeoutFuture::new(32).await;
                        scroll_message_into_view(&first);
                    });
                }
            } else {
                chat_find_cursor.update(|c| {
                    if ids.is_empty() {
                        *c = 0;
                    } else if *c >= ids.len() {
                        *c = ids.len() - 1;
                    }
                });
            }
        }
    });

    // 侧栏「在消息中打开」后滚动到对应气泡。
    Effect::new({
        let focus_message_id_after_nav = focus_message_id_after_nav;
        move |_| {
            let Some(mid) = focus_message_id_after_nav.get() else {
                return;
            };
            focus_message_id_after_nav.set(None);
            let mid = mid.clone();
            spawn_local(async move {
                TimeoutFuture::new(48).await;
                scroll_message_into_view(&mid);
                TimeoutFuture::new(120).await;
                scroll_message_into_view(&mid);
            });
        }
    });

    let attach_chat_stream: Arc<dyn Fn(String, String) + Send + Sync> = Arc::new({
        let abort_cell = Arc::clone(&abort_cell);
        let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
        let sessions = sessions;
        let active_id = active_id;
        let conversation_id = conversation_id;
        let conversation_revision = conversation_revision;
        let selected_agent_role = selected_agent_role;
        let status_busy = status_busy;
        let status_err = status_err;
        let pending_approval = pending_approval;
        let tool_busy = tool_busy;
        let refresh_workspace = Arc::clone(&refresh_workspace);
        let changelist_modal_open = changelist_modal_open;
        let changelist_fetch_nonce = changelist_fetch_nonce;
        move |user_text: String, asst_id: String| {
            if let Some(prev) = abort_cell.lock().unwrap().take() {
                prev.abort();
            }
            *user_cancelled_stream.lock().unwrap() = false;
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            *abort_cell.lock().unwrap() = Some(ac);

            let conv = conversation_id.get();
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();
            let user_cancelled_for_spawn = Arc::clone(&user_cancelled_stream);

            let on_delta: Rc<dyn Fn(String)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                Rc::new(move |chunk: String| {
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act) {
                            if let Some(m) = s.messages.iter_mut().find(|m| m.id == asst_id) {
                                m.text.push_str(&chunk);
                            }
                        }
                    });
                })
            };
            let on_done: Rc<dyn Fn()> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                let abort_cell = Arc::clone(&abort_cell);
                let user_cancelled_stream = Arc::clone(&user_cancelled_for_spawn);
                Rc::new(move || {
                    if *user_cancelled_stream.lock().unwrap() {
                        *abort_cell.lock().unwrap() = None;
                        return;
                    }
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act)
                            && let Some(m) = s.messages.iter_mut().find(|m| m.id == asst_id)
                            && m.state.as_deref() == Some("loading")
                        {
                            // 仅收尾「仍在生成」的气泡；SSE 已 on_error 的勿覆盖 error 状态
                            m.state = None;
                            if m.text.trim().is_empty() {
                                m.text = "(无回复)".to_string();
                            }
                        }
                    });
                    status_busy.set(false);
                    *abort_cell.lock().unwrap() = None;
                })
            };
            let on_error: Rc<dyn Fn(String)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                let abort_cell = Arc::clone(&abort_cell);
                let user_cancelled_stream = Arc::clone(&user_cancelled_for_spawn);
                Rc::new(move |msg: String| {
                    if *user_cancelled_stream.lock().unwrap() {
                        *abort_cell.lock().unwrap() = None;
                        return;
                    }
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act) {
                            if let Some(m) = s.messages.iter_mut().find(|m| m.id == asst_id) {
                                m.text = msg;
                                m.state = Some("error".to_string());
                            }
                        }
                    });
                    status_busy.set(false);
                    status_err.set(Some("对话失败".to_string()));
                    *abort_cell.lock().unwrap() = None;
                })
            };
            let on_ws: Rc<dyn Fn()> = {
                let changelist_modal_open = changelist_modal_open;
                let changelist_fetch_nonce = changelist_fetch_nonce;
                let refresh_workspace = Arc::clone(&refresh_workspace);
                Rc::new(move || {
                    refresh_workspace();
                    if changelist_modal_open.get_untracked() {
                        changelist_fetch_nonce.update(|x| *x = x.wrapping_add(1));
                    }
                })
            };
            let on_tool_status: Rc<dyn Fn(bool)> = {
                let tool_busy = tool_busy;
                Rc::new(move |b: bool| {
                    tool_busy.set(b);
                })
            };
            let on_tool_result: Rc<dyn Fn(ToolResultInfo)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                Rc::new(move |info: ToolResultInfo| {
                    let t = tool_card_text(&info);
                    let id = make_message_id();
                    sessions.update(|list| {
                        if let Some(s) = list.iter_mut().find(|s| s.id == aid_act) {
                            s.messages.push(StoredMessage {
                                id,
                                role: "system".to_string(),
                                text: t,
                                state: None,
                                is_tool: true,
                                created_at: message_created_ms(),
                            });
                        }
                    });
                })
            };
            let on_approval: Rc<dyn Fn(CommandApprovalRequest)> = {
                let pending_approval = pending_approval;
                let sid = appr_store.clone();
                Rc::new(move |req: CommandApprovalRequest| {
                    pending_approval.set(Some((sid.clone(), req.command, req.args)));
                })
            };
            let on_cid: Rc<dyn Fn(String)> = {
                let conversation_id = conversation_id;
                let conversation_revision = conversation_revision;
                Rc::new(move |id: String| {
                    conversation_id.set(Some(id));
                    conversation_revision.set(None);
                })
            };
            let on_conv_rev: Rc<dyn Fn(u64)> = {
                let conversation_revision = conversation_revision;
                Rc::new(move |rev: u64| {
                    conversation_revision.set(Some(rev));
                })
            };

            let cbs = ChatStreamCallbacks {
                on_delta,
                on_done: on_done.clone(),
                on_error: on_error.clone(),
                on_workspace_changed: on_ws,
                on_tool_status,
                on_tool_result,
                on_approval,
                on_conversation_id: on_cid,
                on_conversation_revision: on_conv_rev,
            };

            spawn_local(async move {
                let stream_result = send_chat_stream(
                    user_text,
                    conv,
                    agent_role,
                    Some(appr_for_stream),
                    &signal,
                    cbs.clone(),
                )
                .await;
                if let Err(e) = stream_result {
                    if *user_cancelled_for_spawn.lock().unwrap() {
                        return;
                    }
                    // `stream stopped`：SSE 控制面已调用 `on_error`，勿再收尾以免覆盖助手气泡。
                    if e == "stream stopped" {
                        return;
                    }
                    status_err.set(Some(e.clone()));
                    on_error(e);
                }
            });
        }
    });

    let run_send_message: Arc<dyn Fn() + Send + Sync> = Arc::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        move || {
            let text = composer_draft_buffer.lock().unwrap().trim().to_string();
            if text.is_empty() || !initialized.get() || status_busy.get() {
                return;
            }
            auto_scroll_chat.set(true);
            let uid = make_message_id();
            let asst_id = make_message_id();
            patch_active_session(sessions, &active_id.get(), |s| {
                let now = message_created_ms();
                let is_first_user_turn =
                    s.messages.iter().filter(|m| m.role == "user").count() == 0;
                s.messages.push(StoredMessage {
                    id: uid.clone(),
                    role: "user".to_string(),
                    text: text.clone(),
                    state: None,
                    is_tool: false,
                    created_at: now,
                });
                s.messages.push(StoredMessage {
                    id: asst_id.clone(),
                    role: "assistant".to_string(),
                    text: String::new(),
                    state: Some("loading".to_string()),
                    is_tool: false,
                    created_at: now,
                });
                if is_first_user_turn && s.title == DEFAULT_CHAT_SESSION_TITLE {
                    s.title = title_from_user_prompt(&text);
                }
                s.draft.clear();
            });
            draft.set(String::new());
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(text, asst_id);
        }
    });

    // 由消息气泡「重试」写入助手消息 id，Effect 中消费并发起流（避免在非 Send 的渲染闭包里捕获 Rc<dyn Fn>）。
    let retry_assistant_target = RwSignal::new(None::<String>);
    // 「从此处重试」截断后写入 (用户原文, 新助手 id)，由 Effect 调用 `attach_chat_stream`。
    let regen_stream_after_truncate = RwSignal::new(None::<(String, String)>);

    Effect::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        move |_| {
            let Some(failed_asst_id) = retry_assistant_target.get() else {
                return;
            };
            // 先消费信号，避免在 `status_busy` 等依赖触发下反复入队同一次重试。
            retry_assistant_target.set(None);
            if !initialized.get() || status_busy.get() {
                return;
            }
            let aid = active_id.get();
            let mut prepared: Option<(String, String)> = None;
            sessions.update(|list| {
                prepared = prepare_retry_failed_assistant_turn(list, &aid, &failed_asst_id);
            });
            let Some((user_text, asst_id)) = prepared else {
                return;
            };
            auto_scroll_chat.set(true);
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(user_text, asst_id);
        }
    });

    Effect::new({
        let attach = Arc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        move |_| {
            let Some((user_text, asst_id)) = regen_stream_after_truncate.get() else {
                return;
            };
            regen_stream_after_truncate.set(None);
            if !initialized.get() || status_busy.get() {
                return;
            }
            auto_scroll_chat.set(true);
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);
            attach(user_text, asst_id);
        }
    });

    let cancel_stream: Arc<dyn Fn() + Send + Sync> =
        Arc::new({
            let abort_cell = Arc::clone(&abort_cell);
            let user_cancelled_stream = Arc::clone(&user_cancelled_stream);
            move || {
                if abort_cell.lock().unwrap().is_none() {
                    return;
                }
                *user_cancelled_stream.lock().unwrap() = true;
                if let Some(ac) = abort_cell.lock().unwrap().take() {
                    ac.abort();
                }
                let aid = active_id.get();
                sessions.update(|list| {
                    if let Some(s) = list.iter_mut().find(|s| s.id == aid) {
                        if let Some(m) = s.messages.iter_mut().rev().find(|m| {
                            m.role == "assistant" && m.state.as_deref() == Some("loading")
                        }) {
                            m.state = None;
                            if m.text.trim().is_empty() {
                                m.text = "已停止".to_string();
                            } else {
                                m.text.push_str("\n\n[已停止]");
                            }
                        }
                    }
                });
                status_busy.set(false);
                tool_busy.set(false);
            }
        });

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

    let new_session = {
        let composer_draft_buffer = Arc::clone(&composer_draft_buffer);
        move || {
            let prev = active_id.get_untracked();
            if !prev.is_empty() {
                let buf = composer_draft_buffer.lock().unwrap().clone();
                flush_composer_draft_to_session(sessions, &prev, &buf);
            }
            let now = js_sys::Date::now() as i64;
            let s = ChatSession {
                id: make_session_id(),
                title: DEFAULT_CHAT_SESSION_TITLE.to_string(),
                draft: String::new(),
                messages: vec![],
                updated_at: now,
            };
            let id = s.id.clone();
            sessions.update(|list| {
                list.insert(0, s);
            });
            active_id.set(id);
            draft.set(String::new());
            conversation_id.set(None);
            conversation_revision.set(None);
        }
    };

    let side_resize_session: Rc<RefCell<Option<(f64, f64)>>> = Rc::new(RefCell::new(None));
    let side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>> =
        Rc::new(RefCell::new(None));
    let side_resize_dragging = RwSignal::new(false);

    let composer_buf_nav = Arc::clone(&composer_draft_buffer);
    let composer_buf_ta = Arc::clone(&composer_draft_buffer);

    view! {
        <div class="app-root app-shell-ds">
            {sidebar_nav_view(
                mobile_nav_open,
                session_modal,
                new_session.clone(),
                sidebar_session_query,
                global_message_query,
                sessions,
                active_id,
                draft,
                conversation_id,
                conversation_revision,
                focus_message_id_after_nav,
                session_context_menu,
                composer_buf_nav.clone(),
            )}

            <div class="shell-main">
                {mobile_shell_header_view(mobile_nav_open, new_session.clone())}

                <ApprovalBar pending_approval=pending_approval approval_expanded=approval_expanded />

                <Show when=move || chat_find_panel_open.get()>
                    <ChatFindBar
                        chat_find_panel_open=chat_find_panel_open
                        chat_find_query=chat_find_query
                        chat_find_match_ids=chat_find_match_ids
                        chat_find_cursor=chat_find_cursor
                        auto_scroll_chat=auto_scroll_chat
                    />
                </Show>

                <Show when=move || chat_export_ctx_menu.get().is_some()>
                    <ChatExportContextMenu
                        chat_export_ctx_menu=chat_export_ctx_menu
                        bubble_md_select_mode=bubble_md_select_mode
                        bubble_md_selected_ids=bubble_md_selected_ids
                        sessions=sessions
                        active_id=active_id
                    />
                </Show>

                <div
                    class:main-row-resizing=move || side_resize_dragging.get()
                    class="main-row"
                >
                    {chat_column_view(
                        messages_scroller,
                        auto_scroll_chat,
                        messages_scroll_from_effect,
                        last_messages_scroll_top,
                        session_context_menu,
                        chat_export_ctx_menu,
                        chat_find_panel_open,
                        bubble_md_select_mode,
                        bubble_md_selected_ids,
                        sessions,
                        active_id,
                        expanded_long_assistant_ids,
                        chat_find_query,
                        chat_find_match_ids,
                        chat_find_cursor,
                        composer_input_ref,
                        composer_buf_ta.clone(),
                        run_send_message.clone(),
                        Arc::clone(&cancel_stream),
                        status_busy,
                        initialized,
                        conversation_id,
                        conversation_revision,
                        regen_stream_after_truncate,
                        retry_assistant_target,
                        status_err,
                    )}

                    {side_column_view(
                        side_resize_dragging,
                        side_panel_view,
                        side_width,
                        side_resize_session.clone(),
                        side_resize_handles.clone(),
                        view_menu_open,
                        status_bar_visible,
                        settings_modal,
                        workspace_data,
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
                    Arc::clone(&refresh_status),
                )}
            </div>

            {session_list_modal_view(
                session_modal,
                sessions,
                active_id,
                draft,
                conversation_id,
                composer_draft_buffer.clone(),
            )}

            {settings_modal_view(
                settings_modal,
                theme,
                bg_decor,
                status_data,
                llm_api_base_draft,
                llm_model_draft,
                llm_api_key_draft,
                llm_has_saved_key,
                llm_settings_feedback,
                client_llm_storage_tick,
            )}

            {changelist_modal_view(
                changelist_modal_open,
                changelist_modal_loading,
                changelist_modal_err,
                changelist_modal_rev,
                changelist_fetch_nonce,
                changelist_body_ref,
            )}
        </div>
    }
}
