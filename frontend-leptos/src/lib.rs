#![recursion_limit = "256"]
// CSR 宏生成与大量闭包捕获使若干 style lint 噪声偏高；保持与主包 `-D warnings` 分离。
#![allow(clippy::collapsible_if)]
#![allow(clippy::redundant_locals)]
#![allow(clippy::clone_on_copy)]

mod api;
mod sse_dispatch;
mod storage;

use api::{
    ChatStreamCallbacks, TaskItem, TasksData, WorkspaceData, fetch_tasks, fetch_workspace,
    save_tasks, send_chat_stream, submit_chat_approval,
};
use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use std::cell::RefCell;
use std::rc::Rc;
use storage::{
    ChatSession, StoredMessage, ensure_at_least_one, load_sessions, make_session_id, save_sessions,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::sse_dispatch::{CommandApprovalRequest, ToolResultInfo};

const WORKSPACE_WIDTH_KEY: &str = "agent-demo-workspace-width";
const WORKSPACE_VISIBLE_KEY: &str = "agent-demo-workspace-visible";
const TASKS_VISIBLE_KEY: &str = "agent-demo-tasks-visible";
const STATUS_BAR_VISIBLE_KEY: &str = "agent-demo-status-bar-visible";
const THEME_KEY: &str = "crabmate-theme";
const DEFAULT_SIDE_WIDTH: f64 = 280.0;
const MIN_SIDE_WIDTH: f64 = 200.0;
const MAX_SIDE_WIDTH: f64 = 560.0;

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn load_f64_key(key: &str, default: f64) -> f64 {
    let Some(st) = local_storage() else {
        return default;
    };
    let Ok(Some(v)) = st.get_item(key) else {
        return default;
    };
    match v.parse::<f64>() {
        Ok(n) if (MIN_SIDE_WIDTH..=MAX_SIDE_WIDTH).contains(&n) => n,
        _ => default,
    }
}

fn load_bool_key(key: &str, default: bool) -> bool {
    let Some(st) = local_storage() else {
        return default;
    };
    let Ok(Some(v)) = st.get_item(key) else {
        return default;
    };
    !(v == "0" || v == "false")
}

fn store_bool_key(key: &str, v: bool) {
    if let Some(st) = local_storage() {
        let _ = st.set_item(key, if v { "1" } else { "0" });
    }
}

fn store_f64_key(key: &str, v: f64) {
    if let Some(st) = local_storage() {
        let _ = st.set_item(key, &v.to_string());
    }
}

fn make_message_id() -> String {
    storage::make_session_id()
}

fn tool_card_text(info: &ToolResultInfo) -> String {
    let sum = info.summary.as_deref().unwrap_or("").trim();
    let name = info.name.trim();
    let title = if !sum.is_empty() {
        sum.lines().next().unwrap_or(sum).to_string()
    } else if !name.is_empty() {
        format!("工具：{name}")
    } else {
        "工具输出".to_string()
    };
    let mut out = title;
    if !sum.is_empty() {
        out.push_str("\n\n");
        out.push_str(sum);
    }
    out
}

fn approval_session_id() -> String {
    format!(
        "approval_{}_{}",
        js_sys::Date::now() as i64,
        (js_sys::Math::random() * 1e9) as i64
    )
}

fn patch_active_session(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: &str,
    f: impl FnOnce(&mut ChatSession),
) {
    let id = active_id.to_string();
    sessions.update(|list| {
        if let Some(s) = list.iter_mut().find(|s| s.id == id) {
            f(s);
            s.updated_at = js_sys::Date::now() as i64;
        }
    });
}

#[component]
fn App() -> impl IntoView {
    let sessions = RwSignal::new(Vec::<ChatSession>::new());
    let active_id = RwSignal::new(String::new());
    let initialized = RwSignal::new(false);
    let draft = RwSignal::new(String::new());
    let conversation_id = RwSignal::new(None::<String>);
    let workspace_visible = RwSignal::new(load_bool_key(WORKSPACE_VISIBLE_KEY, true));
    let tasks_visible = RwSignal::new(load_bool_key(TASKS_VISIBLE_KEY, false));
    let status_bar_visible = RwSignal::new(load_bool_key(STATUS_BAR_VISIBLE_KEY, true));
    let side_width = RwSignal::new(load_f64_key(WORKSPACE_WIDTH_KEY, DEFAULT_SIDE_WIDTH));
    let theme = RwSignal::new(
        local_storage()
            .and_then(|s| s.get_item(THEME_KEY).ok().flatten())
            .unwrap_or_else(|| "dark".to_string()),
    );
    let status_busy = RwSignal::new(false);
    let status_err = RwSignal::new(None::<String>);
    let tool_busy = RwSignal::new(false);
    let workspace_data = RwSignal::new(None::<WorkspaceData>);
    let workspace_err = RwSignal::new(None::<String>);
    let tasks_data = RwSignal::new(TasksData { items: vec![] });
    let tasks_err = RwSignal::new(None::<String>);
    let pending_approval = RwSignal::new(None::<(String, String, String)>);
    let session_modal = RwSignal::new(false);
    let abort_cell: Rc<RefCell<Option<web_sys::AbortController>>> = Rc::new(RefCell::new(None));
    // 用户点「停止」后为 true，避免异步 on_done / on_error 覆盖已写入的「已停止」文案。
    let user_cancelled_stream: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let messages_scroller = NodeRef::<Div>::new();
    // 为 false 时表示用户已离开底部，流式输出不再强行跟底；滚回底部附近会重新置 true。
    let auto_scroll_chat = RwSignal::new(true);

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
        store_bool_key(WORKSPACE_VISIBLE_KEY, workspace_visible.get());
    });
    Effect::new(move |_| {
        store_bool_key(TASKS_VISIBLE_KEY, tasks_visible.get());
    });
    Effect::new(move |_| {
        store_bool_key(STATUS_BAR_VISIBLE_KEY, status_bar_visible.get());
    });
    Effect::new(move |_| {
        store_f64_key(WORKSPACE_WIDTH_KEY, side_width.get());
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

    let refresh_workspace = {
        move || {
            spawn_local(async move {
                match fetch_workspace(None).await {
                    Ok(d) => {
                        workspace_err.set(None);
                        workspace_data.set(Some(d));
                    }
                    Err(e) => {
                        workspace_err.set(Some(e));
                        workspace_data.set(None);
                    }
                }
            });
        }
    };

    Effect::new(move |_| {
        if workspace_visible.get() && initialized.get() {
            refresh_workspace();
        }
    });

    let refresh_tasks = {
        move || {
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
            });
        }
    };

    Effect::new(move |_| {
        if tasks_visible.get() && initialized.get() {
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
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
            }
            TimeoutFuture::new(0).await;
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
            }
            // 再等一帧：流式换行后布局高度可能在本轮 paint 后才稳定
            TimeoutFuture::new(16).await;
            if let Some(el) = mref.get() {
                el.set_scroll_top(el.scroll_height());
            }
        });
    });

    let run_send_message: Rc<dyn Fn()> = Rc::new({
        let abort_cell = Rc::clone(&abort_cell);
        let user_cancelled_stream = Rc::clone(&user_cancelled_stream);
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
                s.messages.push(StoredMessage {
                    id: uid.clone(),
                    role: "user".to_string(),
                    text: text.clone(),
                    state: None,
                    is_tool: false,
                });
                s.messages.push(StoredMessage {
                    id: asst_id.clone(),
                    role: "assistant".to_string(),
                    text: String::new(),
                    state: Some("loading".to_string()),
                    is_tool: false,
                });
                s.draft.clear();
            });
            draft.set(String::new());
            status_busy.set(true);
            status_err.set(None);
            pending_approval.set(None);

            if let Some(prev) = abort_cell.borrow_mut().take() {
                prev.abort();
            }
            *user_cancelled_stream.borrow_mut() = false;
            let ac = web_sys::AbortController::new().expect("AbortController");
            let signal = ac.signal();
            *abort_cell.borrow_mut() = Some(ac);

            let conv = conversation_id.get();
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
                Rc::new(move |id: String| {
                    conversation_id.set(Some(id));
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
            };

            spawn_local(async move {
                let stream_result =
                    send_chat_stream(text, conv, Some(appr_for_stream), &signal, cbs.clone()).await;
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
                title: "新会话".to_string(),
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
        }
    };

    let theme_toggle = {
        move |_| {
            theme.update(|t| {
                if t == "dark" {
                    *t = "light".to_string();
                } else {
                    *t = "dark".to_string();
                }
            });
        }
    };

    let narrow_side = {
        move |_| {
            side_width.update(|w| {
                *w = (*w - 40.0).clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
            });
        }
    };
    let widen_side = {
        move |_| {
            side_width.update(|w| {
                *w = (*w + 40.0).clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
            });
        }
    };

    view! {
        <div class="app-root">
            <header class="topbar">
                <div class="brand">
                    <span class="brand-mark" aria-hidden="true"></span>
                    <div class="brand-text">
                        <h1>"CrabMate"</h1>
                        <span class="brand-sub">"本地 Agent"</span>
                    </div>
                </div>
                <span class="topbar-spacer"></span>
                <nav class="topbar-actions">
                    <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| session_modal.set(true)>"会话"</button>
                    <button type="button" class="btn btn-secondary btn-sm" on:click=new_session.clone()>"新会话"</button>
                    <button
                        type="button"
                        class="btn btn-ghost btn-sm"
                        class:active=move || workspace_visible.get()
                        on:click=move |_| workspace_visible.update(|v| *v = !*v)
                        title="工作区"
                    >"工作区"</button>
                    <button
                        type="button"
                        class="btn btn-ghost btn-sm"
                        class:active=move || tasks_visible.get()
                        on:click=move |_| tasks_visible.update(|v| *v = !*v)
                        title="任务"
                    >"任务"</button>
                    <button
                        type="button"
                        class="btn btn-ghost btn-sm"
                        class:active=move || status_bar_visible.get()
                        on:click=move |_| status_bar_visible.update(|v| *v = !*v)
                        title="状态栏"
                    >"状态"</button>
                    <button type="button" class="btn btn-ghost btn-sm" on:click=theme_toggle>"主题"</button>
                </nav>
            </header>

            {move || {
                pending_approval.get().map(|(sid, cmd, args)| {
                    let sid_deny = sid.clone();
                    let sid_once = sid.clone();
                    view! {
                        <div class="approval-bar">
                            <div>"需要审批：运行命令"</div>
                            <pre>{cmd}" "{args}</pre>
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

            <div class="main-row">
                <div class="chat-column">
                    <div
                        class="messages"
                        node_ref=messages_scroller
                        on:scroll=move |ev: web_sys::Event| {
                            if let Some(t) = ev.target() {
                                if let Ok(el) = t.dyn_into::<web_sys::HtmlElement>() {
                                    let gap = el.scroll_height()
                                        - el.scroll_top()
                                        - el.client_height();
                                    auto_scroll_chat.set(gap <= 72);
                                }
                            }
                        }
                    >
                        <div class="messages-inner">
                            {move || {
                                let id = active_id.get();
                                sessions.with(|list| {
                                    list.iter()
                                        .find(|s| s.id == id)
                                        .map(|s| s.messages.clone())
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(|m| {
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
                                            let class_final = if err {
                                                format!("{cls} msg-error")
                                            } else if loading {
                                                format!("{cls} msg-loading")
                                            } else {
                                                cls.to_string()
                                            };
                                            view! {
                                                <div class=class_final>
                                                    <span class="msg-body">{m.text.clone()}</span>
                                                    {loading.then(|| {
                                                        view! {
                                                            <span class="typing-dots" aria-hidden="true">
                                                                <span></span>
                                                                <span></span>
                                                                <span></span>
                                                            </span>
                                                        }
                                                    })}
                                                </div>
                                            }
                                        })
                                        .collect_view()
                                })
                            }}
                        </div>
                    </div>
                    <div class="composer">
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
                        <div class="composer-actions">
                            <button
                                type="button"
                                class="btn btn-primary"
                                prop:disabled=move || status_busy.get() || !initialized.get()
                                on:click=send_message.clone()
                            >"发送"</button>
                            <button
                                type="button"
                                class="btn btn-muted"
                                prop:disabled=move || !status_busy.get()
                                on:click=cancel_stream.clone()
                            >"停止"</button>
                        </div>
                    </div>
                </div>

                <Show when=move || workspace_visible.get() || tasks_visible.get()>
                    <div class="side-column" style:width=move || format!("{}px", side_width.get())>
                        <div class="side-toolbar">
                            <button type="button" class="btn btn-icon" title="收窄侧栏" on:click=narrow_side.clone()>"◀"</button>
                            <button type="button" class="btn btn-icon" title="加宽侧栏" on:click=widen_side.clone()>"▶"</button>
                        </div>
                        <div class="side-body">
                            <Show when=move || workspace_visible.get()>
                                <div
                                    class="side-pane"
                                    style:flex="1"
                                    style:min-width=move || {
                                        if tasks_visible.get() {
                                            "180px"
                                        } else {
                                            "0"
                                        }
                                    }
                                >
                                    <div class="side-pane-title">"工作区"</div>
                                    <div class="workspace-path">
                                        {move || workspace_data.get().map(|d| d.path).unwrap_or_default()}
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
                                    <button type="button" class="btn btn-secondary btn-sm side-action" on:click=move |_| refresh_workspace()>"刷新列表"</button>
                                    <ul class="workspace-list">
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
                                                    .map(|e| {
                                                        let mark = if e.is_dir { "dir" } else { "file" };
                                                        view! { <li class=mark>{e.name}</li> }
                                                    })
                                                    .collect_view()
                                                    .into_any()
                                            }
                                        }}
                                    </ul>
                                </div>
                            </Show>
                            <Show when=move || tasks_visible.get()>
                                <div
                                    class="side-pane"
                                    style:flex="1"
                                    style:min-width=move || {
                                        if workspace_visible.get() {
                                            "180px"
                                        } else {
                                            "0"
                                        }
                                    }
                                >
                                    <div class="side-pane-title">"任务清单"</div>
                                    <button type="button" class="btn btn-secondary btn-sm side-action" on:click=move |_| refresh_tasks()>"刷新"</button>
                                    <Show when=move || tasks_err.get().is_some()>
                                        <div class="msg-error">{move || tasks_err.get().unwrap_or_default()}</div>
                                    </Show>
                                    <ul class="tasks-list">
                                        {move || {
                                            tasks_data.get().items.into_iter().map(|t: TaskItem| {
                                                let id = t.id.clone();
                                                let done = t.done;
                                                view! {
                                                    <li>
                                                        <input
                                                            type="checkbox"
                                                            prop:checked=done
                                                            on:change=move |_| toggle_task(id.clone())
                                                        />
                                                        <span>{t.title}</span>
                                                    </li>
                                                }
                                            }).collect_view()
                                        }}
                                    </ul>
                                </div>
                            </Show>
                        </div>
                    </div>
                </Show>
            </div>

            <Show when=move || status_bar_visible.get()>
                <footer class=move || {
                    if status_err.get().is_some() {
                        "status-bar error"
                    } else {
                        "status-bar"
                    }
                }>
                    {move || {
                        if tool_busy.get() {
                            "工具执行中… "
                        } else {
                            ""
                        }
                    }}
                    {move || {
                        if status_busy.get() {
                            "模型生成中…"
                        } else {
                            "就绪"
                        }
                    }}
                    {move || status_err.get().map(|e| format!(" | {e}")).unwrap_or_default()}
                </footer>
            </Show>

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
                            {move || {
                                sessions.get().into_iter().map(|s| {
                                    let id = s.id.clone();
                                    let active = active_id.get() == id;
                                    let title = s.title.clone();
                                    let n = s.messages.len();
                                    view! {
                                        <div class=if active { "session-row active" } else { "session-row" }>
                                            <button
                                                type="button"
                                                class="session-open"
                                                on:click={
                                                    let id = id.clone();
                                                    move |_| {
                                                        active_id.set(id.clone());
                                                        session_modal.set(false);
                                                    }
                                                }
                                            >
                                                <span class="session-title">{title}</span>
                                                <span class="session-meta">{n}" 条"</span>
                                            </button>
                                        </div>
                                    }
                                }).collect_view()
                            }}
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App /> });
}
