//! 主界面：单根 `App`（导航、对话、侧栏、状态栏、模态框与偏好副作用）。

use std::cell::RefCell;
use std::rc::Rc;

use crate::api::{
    ChatStreamCallbacks, StatusData, TaskItem, TasksData, WorkspaceData,
    clear_client_llm_api_key_storage, client_llm_storage_has_api_key, fetch_status, fetch_tasks,
    fetch_workspace_pick, load_client_llm_text_fields_from_storage, persist_client_llm_to_storage,
    post_chat_branch, post_workspace_set, save_tasks, send_chat_stream, submit_chat_approval,
};
use crate::app_prefs::{
    AGENT_ROLE_KEY, AUTO_SCROLL_RESUME_GAP_PX, BG_DECOR_KEY, DEFAULT_SIDE_WIDTH,
    STATUS_BAR_VISIBLE_KEY, SidePanelView, TASKS_VISIBLE_KEY, THEME_KEY, WORKSPACE_VISIBLE_KEY,
    WORKSPACE_WIDTH_KEY, load_bool_key, load_f64_key, load_side_panel_view, local_storage,
    status_bar_effective_api_base, status_bar_effective_model, store_bool_key, store_f64_key,
    store_side_panel_view,
};
use crate::assistant_body::assistant_markdown_collapsible_view;
use crate::message_format::{message_text_for_display, tool_card_text};
use crate::session_modal_row::SessionModalRow;
use crate::session_ops::{
    SessionContextAnchor, approval_session_id, clamp_session_ctx_menu_pos,
    delete_session_after_confirm, export_session_json_for_id, export_session_markdown_for_id,
    format_msg_time_label, make_message_id, message_created_ms, message_role_label,
    patch_active_session, prepare_retry_failed_assistant_turn, title_from_user_prompt,
    truncate_at_user_message_and_prepare_regenerate, truncate_at_user_message_branch_local,
    user_ordinal_for_message_index, write_clipboard_text,
};
use crate::session_search::{
    MESSAGE_SEARCH_MAX_HITS, collect_message_search_hits, normalize_search_query,
    scroll_message_into_view, session_title_matches,
};
use crate::sse_dispatch::{CommandApprovalRequest, ToolResultInfo};
use crate::storage::{
    ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, ensure_at_least_one, load_sessions,
    make_session_id, save_sessions,
};
use crate::workspace_shell::{begin_side_column_resize, reload_workspace_panel};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::WindowListenerHandle;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;
#[component]
pub fn App() -> impl IntoView {
    let sessions = RwSignal::new(Vec::<ChatSession>::new());
    let active_id = RwSignal::new(String::new());
    let initialized = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
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
    let abort_cell: Rc<RefCell<Option<web_sys::AbortController>>> = Rc::new(RefCell::new(None));
    // 用户点「停止」后为 true，避免异步 on_done / on_error 覆盖已写入的「已停止」文案。
    let user_cancelled_stream: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let messages_scroller = NodeRef::<Div>::new();
    // 为 false 时表示用户已离开底部，流式输出不再强行跟底；滚回底部附近会重新置 true。
    let auto_scroll_chat = RwSignal::new(true);
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
    // 从侧栏跳转后滚动到该消息（DOM 就绪后消费）。
    let focus_message_id_after_nav = RwSignal::new(None::<String>);

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

    let refresh_workspace = {
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
    };

    Effect::new(move |_| {
        if matches!(side_panel_view.get(), SidePanelView::Workspace) && initialized.get() {
            refresh_workspace();
        }
    });

    let refresh_tasks = {
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
    };

    let refresh_status = {
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
    };

    Effect::new(move |_| {
        if initialized.get() && status_data.get().is_none() {
            refresh_status();
        }
    });

    Effect::new(move |_| {
        if matches!(side_panel_view.get(), SidePanelView::Tasks) && initialized.get() {
            refresh_tasks();
        }
    });

    Effect::new(move |_| {
        let _ = active_id.get();
        if !initialized.get() {
            return;
        }
        let id = active_id.get();
        sessions.with(|list| {
            if let Some(s) = list.iter().find(|s| s.id == id) {
                draft.set(s.draft.clone());
            }
        });
        conversation_id.set(None);
        conversation_revision.set(None);
        expanded_long_assistant_ids.set(Vec::new());
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
        spawn_local(async move {
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

    let attach_chat_stream: Rc<dyn Fn(String, String)> = Rc::new({
        let abort_cell = Rc::clone(&abort_cell);
        let user_cancelled_stream = Rc::clone(&user_cancelled_stream);
        let sessions = sessions;
        let active_id = active_id;
        let conversation_id = conversation_id;
        let conversation_revision = conversation_revision;
        let selected_agent_role = selected_agent_role;
        let status_busy = status_busy;
        let status_err = status_err;
        let pending_approval = pending_approval;
        let tool_busy = tool_busy;
        let refresh_workspace = refresh_workspace;
        move |user_text: String, asst_id: String| {
            if let Some(prev) = abort_cell.borrow_mut().take() {
                prev.abort();
            }
            *user_cancelled_stream.borrow_mut() = false;
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            *abort_cell.borrow_mut() = Some(ac);

            let conv = conversation_id.get();
            let agent_role = selected_agent_role.get();
            let appr_for_stream = approval_session_id();
            let appr_store = appr_for_stream.clone();
            let user_cancelled_for_spawn = Rc::clone(&user_cancelled_stream);

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
                let abort_cell = Rc::clone(&abort_cell);
                let user_cancelled_stream = Rc::clone(&user_cancelled_for_spawn);
                Rc::new(move || {
                    if *user_cancelled_stream.borrow() {
                        *abort_cell.borrow_mut() = None;
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
                    *abort_cell.borrow_mut() = None;
                })
            };
            let on_error: Rc<dyn Fn(String)> = {
                let sessions = sessions;
                let aid_act = active_id.get();
                let asst_id = asst_id.clone();
                let abort_cell = Rc::clone(&abort_cell);
                let user_cancelled_stream = Rc::clone(&user_cancelled_for_spawn);
                Rc::new(move |msg: String| {
                    if *user_cancelled_stream.borrow() {
                        *abort_cell.borrow_mut() = None;
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
                    *abort_cell.borrow_mut() = None;
                })
            };
            let on_ws: Rc<dyn Fn()> = {
                Rc::new(move || {
                    refresh_workspace();
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
                    if *user_cancelled_for_spawn.borrow() {
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

    let run_send_message: Rc<dyn Fn()> = Rc::new({
        let attach = Rc::clone(&attach_chat_stream);
        let auto_scroll_chat = auto_scroll_chat;
        move || {
            let text = draft.get().trim().to_string();
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
        let attach = Rc::clone(&attach_chat_stream);
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
        let attach = Rc::clone(&attach_chat_stream);
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

    let send_message = {
        let r = Rc::clone(&run_send_message);
        move |_e: web_sys::MouseEvent| {
            r();
        }
    };

    let cancel_stream =
        {
            let abort_cell = Rc::clone(&abort_cell);
            let user_cancelled_stream = Rc::clone(&user_cancelled_stream);
            move |_| {
                if abort_cell.borrow().is_none() {
                    return;
                }
                *user_cancelled_stream.borrow_mut() = true;
                if let Some(ac) = abort_cell.borrow_mut().take() {
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
        };

    let toggle_task = {
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
    };

    let new_session = {
        move |_| {
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

    view! {
        <div class="app-root app-shell-ds">
            <aside class=move || {
                let mut s = String::from("nav-rail");
                if mobile_nav_open.get() {
                    s.push_str(" nav-rail-mobile-open");
                }
                s
            }>
                <div class="nav-rail-brand">
                    <span class="brand-mark" aria-hidden="true"></span>
                    <div class="nav-rail-brand-text">
                        <h1>"CrabMate"</h1>
                        <span class="brand-sub">"本地 Agent"</span>
                    </div>
                </div>
                <button
                    type="button"
                    class="btn btn-primary btn-new-chat-ds"
                    on:click={
                        let new_session = new_session.clone();
                        move |_| {
                            new_session(());
                            mobile_nav_open.set(false);
                        }
                    }
                >
                    "新对话"
                </button>
                <button
                    type="button"
                    class="btn btn-nav-ghost-ds"
                    on:click={
                        move |_| {
                            session_modal.set(true);
                            mobile_nav_open.set(false);
                        }
                    }
                >
                    "管理会话…"
                </button>
                <div class="nav-rail-search">
                    <label class="nav-rail-search-label" for="nav-session-filter">"筛选会话"</label>
                    <input
                        id="nav-session-filter"
                        type="search"
                        class="nav-session-search-input"
                        placeholder="按标题筛选…"
                        prop:value=move || sidebar_session_query.get()
                        on:input=move |ev| {
                            sidebar_session_query.set(event_target_value(&ev));
                        }
                    />
                    <label class="nav-rail-search-label" for="nav-msg-search">"搜索消息"</label>
                    <input
                        id="nav-msg-search"
                        type="search"
                        class="nav-global-search-input"
                        placeholder="全文搜索（本地）…"
                        prop:value=move || global_message_query.get()
                        on:input=move |ev| {
                            global_message_query.set(event_target_value(&ev));
                        }
                    />
                </div>
                <div class="nav-rail-scroll">
                    <div class="nav-rail-scroll-label">"最近"</div>
                    {move || {
                        let needle = normalize_search_query(&sidebar_session_query.get());
                        let msg_needle = normalize_search_query(&global_message_query.get());
                        let mut v: Vec<ChatSession> = sessions
                            .get()
                            .into_iter()
                            .filter(|s| session_title_matches(s, &needle))
                            .collect();
                        v.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
                        let hits = if msg_needle.is_empty() {
                            Vec::new()
                        } else {
                            sessions.with(|list| {
                                collect_message_search_hits(list, &msg_needle, MESSAGE_SEARCH_MAX_HITS)
                            })
                        };
                        let hit_views = if !msg_needle.is_empty() {
                            if hits.is_empty() {
                                view! {
                                    <div class="nav-search-hits-empty" role="status">
                                        "无匹配消息"
                                    </div>
                                }
                                .into_any()
                            } else {
                                hits
                                    .into_iter()
                                    .map(|h| {
                                        let sid = h.session_id.clone();
                                        let mid = h.message_id.clone();
                                        let title = h.session_title.clone();
                                        let snip = h.snippet.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class="nav-search-hit"
                                                on:click=move |_| {
                                                    session_context_menu.set(None);
                                                    active_id.set(sid.clone());
                                                    draft.set(
                                                        sessions.with(|list| {
                                                            list.iter()
                                                                .find(|s| s.id == sid)
                                                                .map(|s| s.draft.clone())
                                                                .unwrap_or_default()
                                                        }),
                                                    );
                                                    conversation_id.set(None);
                                                    conversation_revision.set(None);
                                                    focus_message_id_after_nav.set(Some(mid.clone()));
                                                    mobile_nav_open.set(false);
                                                }
                                            >
                                                <span class="nav-search-hit-title">{title}</span>
                                                <span class="nav-search-hit-snippet">{snip}</span>
                                            </button>
                                        }
                                    })
                                    .collect_view()
                                    .into_any()
                            }
                        } else {
                            view! { <></> }.into_any()
                        };
                        view! {
                            <div class="nav-search-hits" role="region" aria-label="消息搜索结果">
                                {hit_views}
                            </div>
                            {v.into_iter()
                            .map(|s| {
                                let session_id_class = s.id.clone();
                                let session_id_click = s.id.clone();
                                let session_id_ctx = s.id.clone();
                                let title = s.title.clone();
                                let n = s.messages.len();
                                view! {
                                    <button
                                        type="button"
                                        class=move || {
                                            if active_id.get() == session_id_class {
                                                "nav-session-item is-active"
                                            } else {
                                                "nav-session-item"
                                            }
                                        }
                                        on:contextmenu=move |ev: web_sys::MouseEvent| {
                                            ev.prevent_default();
                                            ev.stop_propagation();
                                            let (x, y) = clamp_session_ctx_menu_pos(
                                                ev.client_x(),
                                                ev.client_y(),
                                            );
                                            session_context_menu.set(Some(SessionContextAnchor {
                                                session_id: session_id_ctx.clone(),
                                                x,
                                                y,
                                            }));
                                        }
                                        on:click={
                                            let id = session_id_click;
                                            move |_| {
                                                session_context_menu.set(None);
                                                active_id.set(id.clone());
                                                draft.set(
                                                    sessions.with(|list| {
                                                        list.iter()
                                                            .find(|s| s.id == id)
                                                            .map(|s| s.draft.clone())
                                                            .unwrap_or_default()
                                                    }),
                                                );
                                                conversation_id.set(None);
                                                conversation_revision.set(None);
                                                mobile_nav_open.set(false);
                                            }
                                        }
                                    >
                                        <span class="nav-session-title">{title}</span>
                                        <span class="nav-session-meta">{n}" 条"</span>
                                    </button>
                                }
                            })
                            .collect_view()}
                        }
                        .into_any()
                    }}
                </div>
            </aside>

            <Show when=move || session_context_menu.get().is_some()>
                <div class="session-ctx-layer">
                <div
                    class="session-ctx-backdrop"
                    aria-hidden="true"
                    on:click=move |_| session_context_menu.set(None)
                ></div>
                <div
                    class="session-ctx-menu"
                    role="menu"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    style=move || {
                        session_context_menu
                            .get()
                            .map(|a| format!("left:{}px;top:{}px;", a.x, a.y))
                            .unwrap_or_default()
                    }
                >
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let anchor = session_context_menu.get();
                            let Some(a) = anchor else {
                                return;
                            };
                            let id = a.session_id;
                            session_context_menu.set(None);
                            export_session_json_for_id(sessions, &id);
                        }
                    >
                        "导出 JSON"
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let anchor = session_context_menu.get();
                            let Some(a) = anchor else {
                                return;
                            };
                            let id = a.session_id;
                            session_context_menu.set(None);
                            export_session_markdown_for_id(sessions, &id);
                        }
                    >
                        "导出 Markdown"
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item session-ctx-item-danger"
                        role="menuitem"
                        on:click=move |_| {
                            let anchor = session_context_menu.get();
                            let Some(a) = anchor else {
                                return;
                            };
                            let id = a.session_id;
                            session_context_menu.set(None);
                            delete_session_after_confirm(
                                sessions,
                                active_id,
                                draft,
                                conversation_id,
                                &id,
                            );
                        }
                    >
                        "删除会话"
                    </button>
                </div>
                </div>
            </Show>

            <Show when=move || mobile_nav_open.get()>
                <div
                    class="nav-rail-backdrop"
                    aria-hidden="true"
                    on:click=move |_| mobile_nav_open.set(false)
                ></div>
            </Show>

            <div class="shell-main">
                <div class="shell-main-header-mobile">
                    <button
                        type="button"
                        class="btn btn-icon"
                        aria-label="打开菜单"
                        on:click=move |_| mobile_nav_open.update(|o| *o = !*o)
                    >
                        "☰"
                    </button>
                    <span class="shell-main-header-title">"CrabMate"</span>
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        on:click={
                            let new_session = new_session.clone();
                            move |_| {
                                new_session(());
                                mobile_nav_open.set(false);
                            }
                        }
                    >
                        "新对话"
                    </button>
                </div>

            {move || {
                pending_approval.get().map(|(sid, cmd, args)| {
                    let sid_deny = sid.clone();
                    let sid_once = sid.clone();
                    let preview = format!("{cmd} {args}");
                    let preview_short: String = preview.chars().take(72).collect();
                    let preview_tail = if preview.chars().count() > 72 {
                        "…"
                    } else {
                        ""
                    };
                    view! {
                        <div class="approval-bar">
                            <button
                                type="button"
                                class="approval-bar-toggle"
                                aria-expanded=move || approval_expanded.get()
                                on:click=move |_| approval_expanded.update(|e| *e = !*e)
                            >
                                <span class="approval-bar-toggle-label">"需要审批：运行命令"</span>
                                <span class="approval-bar-toggle-preview">{preview_short}{preview_tail}</span>
                                <span class="approval-bar-chevron" aria-hidden="true">"▾"</span>
                            </button>
                            <div class=move || {
                                if approval_expanded.get() {
                                    "approval-bar-detail"
                                } else {
                                    "approval-bar-detail approval-bar-detail-collapsed"
                                }
                            }>
                                <pre>{cmd}" "{args}</pre>
                            </div>
                            <div class="actions">
                                <button type="button" class="btn btn-danger btn-sm" on:click={
                                    let sid = sid_deny;
                                    move |_| {
                                        let s = sid.clone();
                                        spawn_local(async move {
                                            let _ = submit_chat_approval(&s, "deny").await;
                                            pending_approval.set(None);
                                        });
                                    }
                                }>"拒绝"</button>
                                <button type="button" class="btn btn-secondary btn-sm" on:click={
                                    let sid = sid_once.clone();
                                    move |_| {
                                        let s = sid.clone();
                                        spawn_local(async move {
                                            let _ = submit_chat_approval(&s, "allow_once").await;
                                            pending_approval.set(None);
                                        });
                                    }
                                }>"允许一次"</button>
                                <button type="button" class="btn btn-primary btn-sm" on:click={
                                    let sid = sid.clone();
                                    move |_| {
                                        let s = sid.clone();
                                        spawn_local(async move {
                                            let _ = submit_chat_approval(&s, "allow_always").await;
                                            pending_approval.set(None);
                                        });
                                    }
                                }>"始终允许"</button>
                            </div>
                        </div>
                    }
                })
            }}

            <Show when=move || chat_find_panel_open.get()>
                <div class="chat-find-wrap">
                    <div class="chat-find-bar" role="search" aria-label="在当前会话中查找">
                                <label class="chat-find-label" for="chat-find-input">"查找"</label>
                                <input
                                    id="chat-find-input"
                                    type="search"
                                    class="chat-find-input"
                                    placeholder="当前会话消息…"
                                    prop:value=move || chat_find_query.get()
                                    on:input=move |ev| {
                                        chat_find_query.set(event_target_value(&ev));
                                    }
                                />
                                <span class="chat-find-meta" aria-live="polite">
                                    {move || {
                                        let q = chat_find_query.get();
                                        if q.trim().is_empty() {
                                            return String::new();
                                        }
                                        let n = chat_find_match_ids.with(|v| v.len());
                                        let c = chat_find_cursor.get();
                                        if n == 0 {
                                            "无匹配".to_string()
                                        } else {
                                            format!("{} / {}", c + 1, n)
                                        }
                                    }}
                                </span>
                                <button
                                    type="button"
                                    class="btn btn-muted btn-sm chat-find-nav"
                                    title="上一条匹配"
                                    prop:disabled=move || {
                                        chat_find_query.get().trim().is_empty()
                                            || chat_find_match_ids.with(|v| v.is_empty())
                                    }
                                    on:click=move |_| {
                                        let ids = chat_find_match_ids.get();
                                        if ids.is_empty() {
                                            return;
                                        }
                                        auto_scroll_chat.set(false);
                                        chat_find_cursor.update(|i| {
                                            if *i == 0 {
                                                *i = ids.len() - 1;
                                            } else {
                                                *i -= 1;
                                            }
                                        });
                                        let idx = chat_find_cursor.get();
                                        scroll_message_into_view(&ids[idx]);
                                    }
                                >
                                    "↑"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn-muted btn-sm chat-find-nav"
                                    title="下一条匹配"
                                    prop:disabled=move || {
                                        chat_find_query.get().trim().is_empty()
                                            || chat_find_match_ids.with(|v| v.is_empty())
                                    }
                                    on:click=move |_| {
                                        let ids = chat_find_match_ids.get();
                                        if ids.is_empty() {
                                            return;
                                        }
                                        auto_scroll_chat.set(false);
                                        chat_find_cursor.update(|i| {
                                            *i = (*i + 1) % ids.len();
                                        });
                                        let idx = chat_find_cursor.get();
                                        scroll_message_into_view(&ids[idx]);
                                    }
                                >
                                    "↓"
                                </button>
                                <button
                                    type="button"
                                    class="btn btn-muted btn-sm chat-find-close"
                                    title="收起查找栏"
                                    aria-label="收起查找栏"
                                    on:click=move |_| chat_find_panel_open.set(false)
                                >
                                    "×"
                                </button>
                    </div>
                </div>
            </Show>

            <div
                class:main-row-resizing=move || side_resize_dragging.get()
                class="main-row"
            >
                <div class="chat-column">
                    <Show when=move || !chat_find_panel_open.get()>
                        <button
                            type="button"
                            class="chat-find-toggle"
                            title="在当前会话中查找"
                            aria-label="在当前会话中查找"
                            aria-expanded="false"
                            on:click=move |_| chat_find_panel_open.set(true)
                        >
                            <svg
                                class="chat-find-icon"
                                viewBox="0 0 24 24"
                                fill="none"
                                stroke="currentColor"
                                stroke-width="2"
                                stroke-linecap="round"
                                stroke-linejoin="round"
                                aria-hidden="true"
                            >
                                <circle cx="11" cy="11" r="8" />
                                <path d="m21 21-4.3-4.3" />
                            </svg>
                        </button>
                    </Show>
                    <div
                        class="messages"
                        node_ref=messages_scroller
                        on:wheel=move |ev: web_sys::WheelEvent| {
                            // 用户上滚查看历史时，立即关闭自动跟底，避免流式期间被强行拉回底部。
                            if ev.delta_y() < 0.0 {
                                auto_scroll_chat.set(false);
                            }
                        }
                        on:scroll=move |ev: web_sys::Event| {
                            if let Some(t) = ev.target() {
                                if let Ok(el) = t.dyn_into::<web_sys::HtmlElement>() {
                                    let top = el.scroll_top();
                                    let prev_top = last_messages_scroll_top.get_untracked();
                                    last_messages_scroll_top.set(top);
                                    let gap = el.scroll_height()
                                        - top
                                        - el.client_height();
                                    if gap > AUTO_SCROLL_RESUME_GAP_PX {
                                        auto_scroll_chat.set(false);
                                    } else if !auto_scroll_chat.get_untracked() && top >= prev_top {
                                        // 仅在向下滚且回到底部附近时恢复自动跟底。
                                        auto_scroll_chat.set(true);
                                    }
                                }
                            }
                        }
                    >
                        <div class="chat-thread">
                        <div class="messages-inner">
                            {move || {
                                let id = active_id.get();
                                sessions.with(|list| {
                                    let msgs = list
                                        .iter()
                                        .find(|s| s.id == id)
                                        .map(|s| s.messages.clone())
                                        .unwrap_or_default();
                                    if msgs.is_empty() {
                                        view! {
                                            <div class="messages-empty" role="status">
                                                <div class="messages-empty-card">
                                                    <p class="messages-empty-title">"开始对话"</p>
                                                    <p class="messages-empty-lead">
                                                        "在下方输入消息，Enter 发送，Shift+Enter 换行。"
                                                    </p>
                                                    <ul class="messages-empty-tips">
                                                        <li>"左侧可新建对话、切换最近会话，或「管理会话」导出与重命名。"</li>
                                                        <li>"侧栏展开时工具栏在右列顶部；「隐藏侧栏」后右侧贴边纵向三键，同宽铺满一条，无额外围框。视图菜单可在隐藏、工作区、任务之间切换。"</li>
                                                    </ul>
                                                </div>
                                            </div>
                                        }
                                        .into_any()
                                    } else {
                                        msgs
                                            .into_iter()
                                            .enumerate()
                                            .map(|(msg_idx, m)| {
                                                let cls = match m.role.as_str() {
                                                    "user" => "msg msg-user",
                                                    "assistant" if m.is_tool => "msg msg-tool",
                                                    "assistant" => "msg msg-assistant",
                                                    _ if m.is_tool => "msg msg-tool",
                                                    _ => "msg msg-system",
                                                };
                                                let loading = m.role == "assistant"
                                                    && m.state.as_deref() == Some("loading");
                                                let err = m.state.as_deref() == Some("error");
                                                let class_prefix = if err {
                                                    format!("{cls} msg-error")
                                                } else if loading {
                                                    format!("{cls} msg-loading")
                                                } else {
                                                    cls.to_string()
                                                };
                                                let mid_highlight = m.id.clone();
                                                let role_lbl = message_role_label(&m);
                                                let time_str =
                                                    format_msg_time_label(m.created_at).unwrap_or_default();
                                                let mid_retry = m.id.clone();
                                                let copy_id = m.id.clone();
                                                let user_retry_id = m.id.clone();
                                                let user_branch_id = m.id.clone();
                                                let is_user_plain = m.role == "user" && !m.is_tool;
                                                let msg_core = if m.role == "assistant" && !m.is_tool {
                                                    assistant_markdown_collapsible_view(
                                                        sessions,
                                                        active_id,
                                                        m.id.clone(),
                                                        expanded_long_assistant_ids,
                                                    )
                                                    .into_any()
                                                } else {
                                                    view! {
                                                        <span class="msg-body">
                                                            {message_text_for_display(&m)}
                                                        </span>
                                                    }
                                                    .into_any()
                                                };
                                                view! {
                                                    <div
                                                        class=move || {
                                                            let mut c = class_prefix.clone();
                                                            if !chat_find_query.get().trim().is_empty() {
                                                                let cur = chat_find_cursor.get();
                                                                let hit = chat_find_match_ids.with(|ids| {
                                                                    ids
                                                                        .get(cur)
                                                                        .map(|x| x == &mid_highlight)
                                                                        .unwrap_or(false)
                                                                });
                                                                if hit {
                                                                    c.push_str(" msg-find-highlight");
                                                                }
                                                            }
                                                            c
                                                        }
                                                        id=format!("msg-{}", m.id)
                                                    >
                                                        <div class="msg-meta" aria-hidden="true">
                                                            <span class="msg-meta-role">{role_lbl}</span>
                                                            <span class="msg-meta-time">{time_str}</span>
                                                        </div>
                                                        {msg_core}
                                                        <div class="msg-actions" role="group" aria-label="消息操作">
                                                            <button
                                                                type="button"
                                                                class="btn btn-muted btn-sm msg-action-btn msg-copy-btn"
                                                                title="复制本条展示文本"
                                                                aria-label="复制本条展示文本"
                                                                on:click=move |_| {
                                                                    let t = sessions.with(|list| {
                                                                        let aid = active_id.get_untracked();
                                                                        list.iter()
                                                                            .find(|s| s.id == aid)
                                                                            .and_then(|s| {
                                                                                s.messages
                                                                                    .iter()
                                                                                    .find(|msg| msg.id == copy_id)
                                                                            })
                                                                            .map(message_text_for_display)
                                                                            .unwrap_or_default()
                                                                    });
                                                                    write_clipboard_text(&t);
                                                                }
                                                            >
                                                                <svg
                                                                    class="msg-copy-icon"
                                                                    viewBox="0 0 24 24"
                                                                    fill="none"
                                                                    xmlns="http://www.w3.org/2000/svg"
                                                                    aria-hidden="true"
                                                                >
                                                                    <rect
                                                                        x="9"
                                                                        y="9"
                                                                        width="13"
                                                                        height="13"
                                                                        rx="2"
                                                                        stroke="currentColor"
                                                                        stroke-width="2"
                                                                    />
                                                                    <path
                                                                        d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1"
                                                                        stroke="currentColor"
                                                                        stroke-width="2"
                                                                    />
                                                                </svg>
                                                            </button>
                                                            {is_user_plain.then(|| {
                                                                let idx = msg_idx;
                                                                let uid_r = user_retry_id.clone();
                                                                let uid_b = user_branch_id.clone();
                                                                view! {
                                                                    <button
                                                                        type="button"
                                                                        class="btn btn-muted btn-sm msg-action-btn"
                                                                        title="删除本条及之后消息并重新生成（服务端会话需已持久化）"
                                                                        prop:disabled=move || status_busy.get()
                                                                        on:click=move |_| {
                                                                            if status_busy.get() {
                                                                                return;
                                                                            }
                                                                            let cid = conversation_id.get();
                                                                            let rev = conversation_revision.get();
                                                                            let ord = sessions.with(|list| {
                                                                                let aid = active_id.get_untracked();
                                                                                list.iter()
                                                                                    .find(|s| s.id == aid)
                                                                                    .and_then(|s| {
                                                                                        user_ordinal_for_message_index(
                                                                                            &s.messages,
                                                                                            idx,
                                                                                        )
                                                                                    })
                                                                            });
                                                                            let uid = uid_r.clone();
                                                                            match (cid, rev, ord) {
                                                                                (
                                                                                    Some(conv),
                                                                                    Some(exp_rev),
                                                                                    Some(before_ord),
                                                                                ) => {
                                                                                    spawn_local(async move {
                                                                                        match post_chat_branch(
                                                                                            &conv,
                                                                                            before_ord,
                                                                                            exp_rev,
                                                                                        )
                                                                                        .await
                                                                                        {
                                                                                            Ok(new_rev) => {
                                                                                                conversation_revision
                                                                                                    .set(Some(new_rev));
                                                                                                let mut prep: Option<
                                                                                                    (String, String),
                                                                                                > = None;
                                                                                                sessions.update(|list| {
                                                                                                    let aid = active_id
                                                                                                        .get_untracked();
                                                                                                    prep = truncate_at_user_message_and_prepare_regenerate(
                                                                                                        list,
                                                                                                        &aid,
                                                                                                        &uid,
                                                                                                    );
                                                                                                });
                                                                                                if let Some((ut, aid)) =
                                                                                                    prep
                                                                                                {
                                                                                                    regen_stream_after_truncate
                                                                                                        .set(Some((ut, aid)));
                                                                                                }
                                                                                            }
                                                                                            Err(e) => {
                                                                                                status_err.set(Some(e));
                                                                                            }
                                                                                        }
                                                                                    });
                                                                                }
                                                                                _ => {
                                                                                    let mut prep: Option<
                                                                                        (String, String),
                                                                                    > = None;
                                                                                    sessions.update(|list| {
                                                                                        let aid = active_id
                                                                                            .get_untracked();
                                                                                        prep = truncate_at_user_message_and_prepare_regenerate(
                                                                                            list,
                                                                                            &aid,
                                                                                            &uid,
                                                                                        );
                                                                                    });
                                                                                    if let Some((ut, aid)) = prep {
                                                                                        regen_stream_after_truncate
                                                                                            .set(Some((ut, aid)));
                                                                                    }
                                                                                }
                                                                            }
                                                                        }
                                                                    >
                                                                        "从此处重试"
                                                                    </button>
                                                                    <button
                                                                        type="button"
                                                                        class="btn btn-muted btn-sm msg-action-btn"
                                                                        title="删除本条及之后消息（不自动发送；服务端会话同步截断需已持久化）"
                                                                        prop:disabled=move || status_busy.get()
                                                                        on:click=move |_| {
                                                                            if status_busy.get() {
                                                                                return;
                                                                            }
                                                                            let cid = conversation_id.get();
                                                                            let rev = conversation_revision.get();
                                                                            let ord = sessions.with(|list| {
                                                                                let aid = active_id.get_untracked();
                                                                                list.iter()
                                                                                    .find(|s| s.id == aid)
                                                                                    .and_then(|s| {
                                                                                        user_ordinal_for_message_index(
                                                                                            &s.messages,
                                                                                            idx,
                                                                                        )
                                                                                    })
                                                                            });
                                                                            let uid = uid_b.clone();
                                                                            match (cid, rev, ord) {
                                                                                (
                                                                                    Some(conv),
                                                                                    Some(exp_rev),
                                                                                    Some(before_ord),
                                                                                ) => {
                                                                                    spawn_local(async move {
                                                                                        match post_chat_branch(
                                                                                            &conv,
                                                                                            before_ord,
                                                                                            exp_rev,
                                                                                        )
                                                                                        .await
                                                                                        {
                                                                                            Ok(new_rev) => {
                                                                                                conversation_revision
                                                                                                    .set(Some(new_rev));
                                                                                                sessions.update(|list| {
                                                                                                    let aid = active_id
                                                                                                        .get_untracked();
                                                                                                    let _ = truncate_at_user_message_branch_local(
                                                                                                        list,
                                                                                                        &aid,
                                                                                                        &uid,
                                                                                                    );
                                                                                                });
                                                                                            }
                                                                                            Err(e) => {
                                                                                                status_err.set(Some(e));
                                                                                            }
                                                                                        }
                                                                                    });
                                                                                }
                                                                                _ => {
                                                                                    sessions.update(|list| {
                                                                                        let aid = active_id
                                                                                            .get_untracked();
                                                                                        let _ = truncate_at_user_message_branch_local(
                                                                                            list,
                                                                                            &aid,
                                                                                            &uid,
                                                                                        );
                                                                                    });
                                                                                }
                                                                            }
                                                                        }
                                                                    >
                                                                        "分支对话"
                                                                    </button>
                                                                }
                                                            })}
                                                        </div>
                                                        {loading.then(|| {
                                                            view! {
                                                                <span class="typing-dots" aria-hidden="true">
                                                                    <span></span>
                                                                    <span></span>
                                                                    <span></span>
                                                                </span>
                                                            }
                                                        })}
                                                        {err.then(move || {
                                                            let mid = mid_retry.clone();
                                                            view! {
                                                                <div class="msg-retry-row">
                                                                    <button
                                                                        type="button"
                                                                        class="btn btn-secondary btn-sm"
                                                                        prop:disabled=move || status_busy.get()
                                                                        on:click=move |_| {
                                                                            retry_assistant_target.set(Some(mid.clone()));
                                                                        }
                                                                    >
                                                                        "重试"
                                                                    </button>
                                                                </div>
                                                            }
                                                        })}
                                                    </div>
                                                }
                                            })
                                            .collect_view()
                                            .into_any()
                                    }
                                })
                            }}
                        </div>
                        </div>
                    </div>
                    <div class="composer composer-ds">
                        <div class="composer-inner-ds">
                        <textarea
                            class="composer-input"
                            prop:value=move || draft.get()
                            on:input=move |ev| {
                                let v = event_target_value(&ev);
                                draft.set(v.clone());
                                patch_active_session(sessions, &active_id.get(), |s| {
                                    s.draft = v;
                                });
                            }
                            on:keydown={
                                let r = Rc::clone(&run_send_message);
                                move |ev: web_sys::KeyboardEvent| {
                                    if ev.key() == "Enter" && !ev.shift_key() {
                                        ev.prevent_default();
                                        r();
                                    }
                                }
                            }
                            placeholder="输入消息，Enter 发送 / Shift+Enter 换行…"
                            rows="3"
                        ></textarea>
                        <div class="composer-bar-actions">
                            <button
                                type="button"
                                class="btn btn-muted btn-sm"
                                prop:disabled=move || !status_busy.get()
                                on:click=cancel_stream.clone()
                            >"停止"</button>
                            <button
                                type="button"
                                class="btn btn-primary btn-send-icon"
                                prop:disabled=move || status_busy.get() || !initialized.get()
                                on:click=send_message.clone()
                                title="发送"
                            >"➤"</button>
                        </div>
                        </div>
                    </div>
                </div>

                <div
                    class="column-resize-handle"
                    class:column-resize-handle-off=move || {
                        matches!(side_panel_view.get(), SidePanelView::None)
                    }
                    role="separator"
                    aria-orientation="vertical"
                    aria-label="拖拽调整右列宽度"
                    on:mousedown={
                        let sess = Rc::clone(&side_resize_session);
                        let hands = Rc::clone(&side_resize_handles);
                        move |ev| {
                            begin_side_column_resize(
                                ev,
                                side_panel_view,
                                side_width,
                                side_resize_dragging,
                                Rc::clone(&sess),
                                Rc::clone(&hands),
                            );
                        }
                    }
                ></div>

                <div
                    class:side-column-resizing=move || side_resize_dragging.get()
                    class=move || {
                        let mut c = String::from("side-column");
                        if matches!(side_panel_view.get(), SidePanelView::None) {
                            c.push_str(" side-column-rail-only");
                        }
                        c
                    }
                    style:width=move || {
                        if matches!(side_panel_view.get(), SidePanelView::None) {
                            "0px".to_string()
                        } else {
                            format!("{}px", side_width.get())
                        }
                    }
                >
                        <div class="shell-main-toolbar" role="toolbar" aria-label="视图与设置">
                            <div class="toolbar-view-wrap">
                                <Show when=move || view_menu_open.get()>
                                    <div
                                        class="toolbar-view-backdrop"
                                        on:click=move |_| view_menu_open.set(false)
                                    ></div>
                                </Show>
                                <button
                                    type="button"
                                    class="btn btn-secondary btn-sm toolbar-view-trigger"
                                    class:active=move || !matches!(side_panel_view.get(), SidePanelView::None)
                                    class:toolbar-view-trigger-open=move || view_menu_open.get()
                                    on:click=move |_| view_menu_open.update(|o| *o = !*o)
                                    title="选择侧栏：隐藏 / 工作区 / 任务"
                                >
                                    {move || {
                                        let suffix = if view_menu_open.get() { "▴" } else { "▾" };
                                        format!("视图{suffix}")
                                    }}
                                </button>
                                <Show when=move || view_menu_open.get()>
                                    <div class="toolbar-view-menu" role="menu" aria-label="侧栏视图">
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::None)
                                            role="menuitem"
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::None);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            "隐藏侧栏"
                                        </button>
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::Workspace)
                                            role="menuitem"
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::Workspace);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            "工作区"
                                        </button>
                                        <button
                                            type="button"
                                            class="toolbar-view-menu-item"
                                            class:active=move || matches!(side_panel_view.get(), SidePanelView::Tasks)
                                            role="menuitem"
                                            on:click=move |_| {
                                                side_panel_view.set(SidePanelView::Tasks);
                                                view_menu_open.set(false);
                                            }
                                        >
                                            "任务"
                                        </button>
                                    </div>
                                </Show>
                            </div>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm"
                                class:active=move || status_bar_visible.get()
                                on:click=move |_| status_bar_visible.update(|v| *v = !*v)
                                title="状态栏"
                            >
                                "状态"
                            </button>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm"
                                on:click=move |_| settings_modal.set(true)
                                title="外观与背景"
                            >
                                "设置"
                            </button>
                        </div>
                        <div class="side-body">
                            <Show when=move || matches!(side_panel_view.get(), SidePanelView::Workspace)>
                                <div class="side-pane" style:flex="1" style:min-width="0">
                                    <div class="side-card">
                                        <Show when=move || {
                                            workspace_loading.get()
                                                || workspace_err.get().is_some()
                                                || workspace_data
                                                    .get()
                                                    .and_then(|d| d.error.clone())
                                                    .is_some()
                                        }>
                                            <div class="side-card-head">
                                                <div class="side-head-main">
                                                    <span class="side-head-stat">{move || {
                                                        if workspace_loading.get() {
                                                            "加载中…".to_string()
                                                        } else {
                                                            "错误".to_string()
                                                        }
                                                    }}</span>
                                                </div>
                                            </div>
                                        </Show>
                                        <div class="side-card-body workspace-side-card-body">
                                            <div class="workspace-side-card-scroll">
                                            {move || {
                                                if workspace_loading.get() {
                                                    view! {
                                                        <div class="skeleton-stack" aria-busy="true" aria-label="加载工作区">
                                                            <div class="skeleton skeleton-block skeleton-ws-path"></div>
                                                            <ul class="workspace-list workspace-list-skeleton">
                                                                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                                                                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                                                                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                                                                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                                                                <li><span class="skeleton skeleton-line skeleton-ws-row"></span></li>
                                                            </ul>
                                                        </div>
                                                    }
                                                    .into_any()
                                                } else {
                                                    view! {
                                                        <div class="side-card-loaded">
                                                            <div class="workspace-set">
                                                                <div class="workspace-set-label">"工作区根目录"</div>
                                                                <div class="workspace-set-input-row">
                                                                    <input
                                                                        type="text"
                                                                        class="workspace-set-input"
                                                                        placeholder="绝对路径，须落在服务端允许根内"
                                                                        prop:value=move || workspace_path_draft.get()
                                                                        on:input=move |ev| {
                                                                            workspace_path_draft
                                                                                .set(event_target_value(&ev));
                                                                        }
                                                                    />
                                                                    <button
                                                                        type="button"
                                                                        class="btn btn-secondary btn-sm workspace-set-browse"
                                                                        title="在运行 serve 的机器上打开系统选目录对话框"
                                                                        prop:disabled=move || {
                                                                            workspace_pick_busy.get()
                                                                                || workspace_loading.get()
                                                                        }
                                                                        on:click=move |_| {
                                                                            workspace_set_err.set(None);
                                                                            workspace_pick_busy.set(true);
                                                                            spawn_local(async move {
                                                                                match fetch_workspace_pick().await {
                                                                                    Ok(Some(p)) => {
                                                                                        workspace_path_draft.set(p);
                                                                                    }
                                                                                    Ok(None) => {
                                                                                        workspace_set_err.set(Some(
                                                                                            "未选择目录，或服务端无法弹窗（无图形/无头/SSH 远端）。请手动填写路径。"
                                                                                                .into(),
                                                                                        ));
                                                                                    }
                                                                                    Err(e) => {
                                                                                        workspace_set_err.set(Some(e));
                                                                                    }
                                                                                }
                                                                                workspace_pick_busy.set(false);
                                                                            });
                                                                        }
                                                                    >
                                                                        {move || {
                                                                            if workspace_pick_busy.get() {
                                                                                "…"
                                                                            } else {
                                                                                "浏览…"
                                                                            }
                                                                        }}
                                                                    </button>
                                                                </div>
                                                                <div class="workspace-set-actions">
                                                                    <button
                                                                        type="button"
                                                                        class="btn btn-primary btn-sm"
                                                                        prop:disabled=move || {
                                                                            workspace_set_busy.get()
                                                                                || workspace_pick_busy.get()
                                                                                || workspace_loading.get()
                                                                        }
                                                                        on:click=move |_| {
                                                                            workspace_set_err.set(None);
                                                                            let p = workspace_path_draft
                                                                                .get()
                                                                                .trim()
                                                                                .to_string();
                                                                            if p.is_empty() {
                                                                                workspace_set_err.set(Some(
                                                                                    "请填写目录路径。".into(),
                                                                                ));
                                                                                return;
                                                                            }
                                                                            workspace_set_busy.set(true);
                                                                            spawn_local(async move {
                                                                                match post_workspace_set(Some(p)).await {
                                                                                    Ok(_) => {
                                                                                        reload_workspace_panel(
                                                                                            workspace_loading,
                                                                                            workspace_err,
                                                                                            workspace_path_draft,
                                                                                            workspace_data,
                                                                                        )
                                                                                        .await;
                                                                                    }
                                                                                    Err(e) => {
                                                                                        workspace_set_err.set(Some(e));
                                                                                    }
                                                                                }
                                                                                workspace_set_busy.set(false);
                                                                            });
                                                                        }
                                                                    >
                                                                        "应用"
                                                                    </button>
                                                                </div>
                                                                <Show when=move || workspace_set_err.get().is_some()>
                                                                    <div class="msg-error workspace-set-error">{move || {
                                                                        workspace_set_err
                                                                            .get()
                                                                            .unwrap_or_default()
                                                                    }}</div>
                                                                </Show>
                                                            </div>
                                                            <Show when=move || {
                                                                workspace_err.get().is_some()
                                                                    || workspace_data.get().and_then(|d| d.error).is_some()
                                                            }>
                                                                <div class="msg-error">{move || {
                                                                    workspace_err
                                                                        .get()
                                                                        .or_else(|| workspace_data.get().and_then(|d| d.error))
                                                                        .unwrap_or_default()
                                                                }}</div>
                                                            </Show>
                                                            <ul class=move || {
                                                                let entries = workspace_data
                                                                    .get()
                                                                    .map(|d| d.entries)
                                                                    .unwrap_or_default();
                                                                if entries.is_empty() {
                                                                    "workspace-list"
                                                                } else {
                                                                    "workspace-list list-stagger"
                                                                }
                                                            }>
                                                                {move || {
                                                                    let entries = workspace_data
                                                                        .get()
                                                                        .map(|d| d.entries)
                                                                        .unwrap_or_default();
                                                                    if entries.is_empty() {
                                                                        view! { <li>"（无数据）"</li> }.into_any()
                                                                    } else {
                                                                        entries
                                                                            .into_iter()
                                                                            .enumerate()
                                                                            .map(|(i, e)| {
                                                                                let mark = if e.is_dir { "dir" } else { "file" };
                                                                                let stagger = i.to_string();
                                                                                view! {
                                                                                    <li
                                                                                        class=mark
                                                                                        style=format!("--list-stagger: {stagger}")
                                                                                    >
                                                                                        {e.name}
                                                                                    </li>
                                                                                }
                                                                            })
                                                                            .collect_view()
                                                                            .into_any()
                                                                    }
                                                                }}
                                                            </ul>
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                            }}
                                            </div>
                                            <div class="workspace-list-refresh">
                                                <button
                                                    type="button"
                                                    class="btn btn-secondary btn-sm workspace-list-refresh-btn"
                                                    on:click=move |_| refresh_workspace()
                                                >
                                                    "刷新列表"
                                                </button>
                                            </div>
                                        </div>
                                    </div>
                                </div>
                            </Show>
                            <Show when=move || matches!(side_panel_view.get(), SidePanelView::Tasks)>
                                <div class="side-pane" style:flex="1" style:min-width="0">
                                    <div class="side-card">
                                        <div class="side-card-head">
                                            <div class="side-head-main">
                                                <div class="side-pane-title">"任务清单"</div>
                                                <span class="side-head-stat">{move || {
                                                    if tasks_loading.get() {
                                                        "加载中…".to_string()
                                                    } else if tasks_err.get().is_some() {
                                                        "错误".to_string()
                                                    } else {
                                                        let items = tasks_data.get().items;
                                                        let total = items.len();
                                                        let done = items.iter().filter(|t| t.done).count();
                                                        format!("{done}/{total} 完成")
                                                    }
                                                }}</span>
                                            </div>
                                            <button type="button" class="btn btn-secondary btn-sm side-head-action" on:click=move |_| refresh_tasks()>"刷新"</button>
                                        </div>
                                        <div class="side-card-body">
                                            {move || {
                                                if tasks_loading.get() {
                                                    view! {
                                                        <div class="skeleton-stack" aria-busy="true" aria-label="加载任务">
                                                            <ul class="tasks-list tasks-list-skeleton">
                                                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                                                <li><span class="skeleton skeleton-task-check"></span><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                                            </ul>
                                                        </div>
                                                    }
                                                    .into_any()
                                                } else {
                                                    view! {
                                                        <div class="side-card-loaded">
                                                            <Show when=move || tasks_err.get().is_some()>
                                                                <div class="msg-error">{move || tasks_err.get().unwrap_or_default()}</div>
                                                            </Show>
                                                            <ul class=move || {
                                                                if tasks_data.get().items.is_empty() {
                                                                    "tasks-list"
                                                                } else {
                                                                    "tasks-list list-stagger"
                                                                }
                                                            }>
                                                                {move || {
                                                                    tasks_data
                                                                        .get()
                                                                        .items
                                                                        .into_iter()
                                                                        .enumerate()
                                                                        .map(|(i, t): (usize, TaskItem)| {
                                                                            let id = t.id.clone();
                                                                            let done = t.done;
                                                                            let stagger = i.to_string();
                                                                            view! {
                                                                                <li style=format!("--list-stagger: {stagger}")>
                                                                                    <input
                                                                                        type="checkbox"
                                                                                        prop:checked=done
                                                                                        on:change=move |_| toggle_task(id.clone())
                                                                                    />
                                                                                    <span>{t.title}</span>
                                                                                </li>
                                                                            }
                                                                        })
                                                                        .collect_view()
                                                                }}
                                                            </ul>
                                                        </div>
                                                    }
                                                    .into_any()
                                                }
                                            }}
                                        </div>
                                    </div>
                                </div>
                            </Show>
                        </div>
                </div>
            </div>

            <Show when=move || status_bar_visible.get()>
                <footer class=move || {
                    if status_fetch_err.get().is_some() {
                        "status-bar status-bar-fetch-error"
                    } else {
                        "status-bar"
                    }
                }>
                    <div class="status-chips">
                        {move || {
                            if status_loading.get() {
                                view! {
                                    <div class="status-chips-skeleton" aria-busy="true" aria-label="加载状态">
                                        <span class="status-chip status-chip-skeleton">
                                            <span class="skeleton skeleton-chip-label"></span>
                                            <span class="skeleton skeleton-chip-value skeleton-chip-model"></span>
                                        </span>
                                        <span class="status-chip status-chip-skeleton status-chip-url">
                                            <span class="skeleton skeleton-chip-label"></span>
                                            <span class="skeleton skeleton-chip-value skeleton-chip-url-bar"></span>
                                        </span>
                                        <span class="status-chip status-chip-skeleton status-chip-role">
                                            <span class="skeleton skeleton-chip-label"></span>
                                            <span class="skeleton skeleton-chip-value skeleton-chip-role-select"></span>
                                        </span>
                                    </div>
                                }
                                .into_any()
                            } else if let Some(fetch_err) = status_fetch_err.get() {
                                view! {
                                    <div
                                        class="status-fetch-error"
                                        role="status"
                                        aria-live="polite"
                                    >
                                        <span class="status-fetch-error-text" title=fetch_err.clone()>
                                            {format!("无法加载状态（/status）：{fetch_err}")}
                                        </span>
                                        <button
                                            type="button"
                                            class="btn btn-secondary btn-sm"
                                            on:click=move |_| refresh_status()
                                        >
                                            "重试"
                                        </button>
                                    </div>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <>
                                        <span class="status-chip">
                                            <span class="status-chip-label">"模型"</span>
                                            <span class="status-chip-value">{move || {
                                                let _tick = client_llm_storage_tick.get();
                                                let sd = status_data.get();
                                                let (_, stored_model) =
                                                    load_client_llm_text_fields_from_storage();
                                                status_bar_effective_model(
                                                    sd.as_ref(),
                                                    stored_model.as_str(),
                                                )
                                            }}</span>
                                        </span>
                                        <span class="status-chip status-chip-url" title=move || {
                                            let _tick = client_llm_storage_tick.get();
                                            let sd = status_data.get();
                                            let (stored_base, _) =
                                                load_client_llm_text_fields_from_storage();
                                            status_bar_effective_api_base(
                                                sd.as_ref(),
                                                stored_base.as_str(),
                                            )
                                        }>
                                            <span class="status-chip-label">"base_url"</span>
                                            <span class="status-chip-value">{move || {
                                                let _tick = client_llm_storage_tick.get();
                                                let sd = status_data.get();
                                                let (stored_base, _stored_model) =
                                                    load_client_llm_text_fields_from_storage();
                                                status_bar_effective_api_base(
                                                    sd.as_ref(),
                                                    stored_base.as_str(),
                                                )
                                            }}</span>
                                        </span>
                                        <label class="status-chip status-chip-role" title="Agent 角色（对标 CLI /agent set）">
                                            <span class="status-chip-label">"角色"</span>
                                            <select
                                                class="status-agent-select"
                                                prop:value=move || {
                                                    selected_agent_role
                                                        .get()
                                                        .unwrap_or_else(|| "__default__".to_string())
                                                }
                                                on:change=move |ev| {
                                                    let v = event_target_value(&ev);
                                                    let t = v.trim();
                                                    if t.is_empty() || t == "__default__" {
                                                        selected_agent_role.set(None);
                                                    } else {
                                                        selected_agent_role.set(Some(t.to_string()));
                                                    }
                                                }
                                            >
                                                <option value="__default__">{move || {
                                                    status_data
                                                        .get()
                                                        .and_then(|d| d.default_agent_role_id)
                                                        .map(|id| format!("default ({id})"))
                                                        .unwrap_or_else(|| "default".to_string())
                                                }}</option>
                                                {move || {
                                                    status_data
                                                        .get()
                                                        .map(|d| d.agent_role_ids)
                                                        .unwrap_or_default()
                                                        .into_iter()
                                                        .map(|id| {
                                                            let label = id.clone();
                                                            view! { <option value=id>{label}</option> }
                                                        })
                                                        .collect_view()
                                                }}
                                            </select>
                                        </label>
                                    </>
                                }
                                .into_any()
                            }
                        }}
                    </div>
                    <span class=move || {
                        let kind = if status_fetch_err.get().is_some() || status_err.get().is_some() {
                            "error"
                        } else if tool_busy.get() {
                            "tool"
                        } else if status_busy.get() {
                            "running"
                        } else {
                            "ready"
                        };
                        format!("status-run status-run-{kind}")
                    }>
                        <span class="status-run-dot" aria-hidden="true"></span>
                        <span>{move || {
                            if status_fetch_err.get().is_some() {
                                "/status 不可用".to_string()
                            } else if let Some(e) = status_err.get() {
                                format!("错误: {e}")
                            } else if tool_busy.get() {
                                "工具执行中…".to_string()
                            } else if status_busy.get() {
                                "模型生成中…".to_string()
                            } else {
                                "就绪".to_string()
                            }
                        }}</span>
                    </span>
                </footer>
            </Show>

            </div>

            <Show when=move || session_modal.get()>
                <div class="modal-backdrop" on:click=move |_| session_modal.set(false)>
                    <div class="modal" on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()>
                        <div class="modal-head">
                            <h2 class="modal-title">"会话"</h2>
                            <span class="modal-badge">"本地"</span>
                            <span class="modal-head-spacer"></span>
                            <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| session_modal.set(false)>"关闭"</button>
                        </div>
                        <div class="modal-body">
                            <p class="modal-hint">
                                "本地保存在浏览器；可导出为与 CLI save-session 同形的 JSON / Markdown 下载。"
                            </p>
                            {move || {
                                sessions
                                    .get()
                                    .into_iter()
                                    .map(|s| {
                                        let id = s.id.clone();
                                        let active = active_id.get() == id;
                                        view! {
                                            <SessionModalRow
                                                id=id.clone()
                                                title=s.title.clone()
                                                message_count=s.messages.len()
                                                active=active
                                                sessions=sessions
                                                active_id=active_id
                                                draft=draft
                                                conversation_id=conversation_id
                                                session_modal=session_modal
                                            />
                                        }
                                    })
                                    .collect_view()
                            }}
                        </div>
                    </div>
                </div>
            </Show>

            <Show when=move || settings_modal.get()>
                <div class="modal-backdrop" on:click=move |_| settings_modal.set(false)>
                    <div
                        class="modal"
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="settings-modal-title"
                        on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    >
                        <div class="modal-head">
                            <h2 class="modal-title" id="settings-modal-title">"设置"</h2>
                            <span class="modal-badge">"本机"</span>
                            <span class="modal-head-spacer"></span>
                            <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| settings_modal.set(false)>
                                "关闭"
                            </button>
                        </div>
                        <div class="modal-body">
                            <p class="modal-hint">"主题与页面背景保存在本机（localStorage）。模型网关与 API 密钥也可仅存本机；发消息时会在 JSON 中附带覆盖项，请仅在可信环境（HTTPS）使用。"</p>
                            <div class="settings-block">
                                <h3 class="settings-block-title">"主题"</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "dark"
                                        on:click=move |_| theme.set("dark".to_string())
                                    >
                                        "深色"
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "light"
                                        on:click=move |_| theme.set("light".to_string())
                                    >
                                        "浅色"
                                    </button>
                                </div>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">"页面背景"</h3>
                                <label class="settings-checkbox-label">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || bg_decor.get()
                                        on:change=move |_| bg_decor.update(|v| *v = !*v)
                                    />
                                    <span>"显示背景光晕（径向渐变）"</span>
                                </label>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">"模型网关（可选覆盖）"</h3>
                                <p class="modal-hint settings-field-nested-hint">
                                    "留空则使用服务端配置与环境变量 API_KEY。API 密钥使用密码框，不会以明文显示。"
                                </p>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-base">
                                        "API 基址（api_base）"
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-api-base"
                                        class="settings-text-input"
                                        placeholder="例如 https://api.deepseek.com/v1"
                                        prop:value=move || llm_api_base_draft.get()
                                        on:input=move |ev| {
                                            llm_api_base_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-model">
                                        "模型名称（model）"
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-model"
                                        class="settings-text-input"
                                        placeholder="例如 deepseek-chat"
                                        prop:value=move || llm_model_draft.get()
                                        on:input=move |ev| {
                                            llm_model_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-key">
                                        "API 密钥（覆盖 API_KEY）"
                                    </label>
                                    <input
                                        type="password"
                                        id="settings-llm-api-key"
                                        class="settings-text-input"
                                        autocomplete="off"
                                        placeholder="留空保留已存密钥；填写新密钥后点保存"
                                        prop:value=move || llm_api_key_draft.get()
                                        on:input=move |ev| {
                                            llm_api_key_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <Show when=move || llm_has_saved_key.get()>
                                    <p class="modal-hint settings-field-nested-hint">
                                        "当前已在本机保存密钥（不会回显到输入框）。"
                                    </p>
                                </Show>
                                <div class="settings-actions-row">
                                    <button
                                        type="button"
                                        class="btn btn-primary btn-sm"
                                        on:click=move |_| {
                                            llm_settings_feedback.set(None);
                                            let key_raw = llm_api_key_draft.get();
                                            let api_key_upd = if key_raw.trim().is_empty() {
                                                None
                                            } else {
                                                Some(key_raw)
                                            };
                                            let base = llm_api_base_draft.get();
                                            let model = llm_model_draft.get();
                                            match persist_client_llm_to_storage(
                                                &base,
                                                &model,
                                                api_key_upd.as_deref(),
                                            ) {
                                                Ok(()) => {
                                                    llm_api_key_draft.set(String::new());
                                                    llm_has_saved_key
                                                        .set(client_llm_storage_has_api_key());
                                                    client_llm_storage_tick
                                                        .update(|n| *n = n.wrapping_add(1));
                                                    llm_settings_feedback.set(Some(
                                                        "已保存到本机浏览器".into(),
                                                    ));
                                                }
                                                Err(e) => llm_settings_feedback.set(Some(e)),
                                            }
                                        }
                                    >
                                        "保存模型设置"
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        prop:disabled=move || !llm_has_saved_key.get()
                                        on:click=move |_| {
                                            llm_settings_feedback.set(None);
                                            let _ = clear_client_llm_api_key_storage();
                                            llm_has_saved_key.set(false);
                                            llm_settings_feedback.set(Some(
                                                "已清除本机保存的密钥".into(),
                                            ));
                                        }
                                    >
                                        "清除已存密钥"
                                    </button>
                                </div>
                                <Show when=move || llm_settings_feedback.get().is_some()>
                                    <p class="settings-save-feedback">{move || {
                                        llm_settings_feedback.get().unwrap_or_default()
                                    }}</p>
                                </Show>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}
