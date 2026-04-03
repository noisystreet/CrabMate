#![recursion_limit = "256"]
// CSR 宏生成与大量闭包捕获使若干 style lint 噪声偏高；保持与主包 `-D warnings` 分离。
#![allow(clippy::collapsible_if)]
#![allow(clippy::redundant_locals)]
#![allow(clippy::clone_on_copy)]

mod api;
mod session_export;
mod sse_dispatch;
mod storage;

use api::{
    ChatStreamCallbacks, StatusData, TaskItem, TasksData, WorkspaceData, fetch_status, fetch_tasks,
    fetch_workspace, fetch_workspace_pick, post_workspace_set, save_tasks, send_chat_stream,
    submit_chat_approval,
};
use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::WindowListenerHandle;
use leptos_dom::helpers::event_target_value;
use leptos_dom::helpers::window_event_listener;
use serde_json::Value;
use session_export::{
    export_filename_stem, session_to_export_file, session_to_markdown, trigger_download,
};
use std::cell::RefCell;
use std::rc::Rc;
use storage::{
    ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, ensure_at_least_one, load_sessions,
    make_session_id, save_sessions,
};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::sse_dispatch::{CommandApprovalRequest, ToolResultInfo};

const WORKSPACE_WIDTH_KEY: &str = "agent-demo-workspace-width";
const WORKSPACE_VISIBLE_KEY: &str = "agent-demo-workspace-visible";
const TASKS_VISIBLE_KEY: &str = "agent-demo-tasks-visible";
const STATUS_BAR_VISIBLE_KEY: &str = "agent-demo-status-bar-visible";
const THEME_KEY: &str = "crabmate-theme";
/// 为 `true` 时显示页面径向渐变光晕；`false` 时仅纯色背景（`data-bg-decor="plain"`）。
const BG_DECOR_KEY: &str = "crabmate-bg-decor";
const AGENT_ROLE_KEY: &str = "agent-demo-agent-role";
const DEFAULT_SIDE_WIDTH: f64 = 280.0;
const MIN_SIDE_WIDTH: f64 = 200.0;
const MAX_SIDE_WIDTH: f64 = 560.0;
/// 为左侧对话列预留的最小宽度（视口过窄时仍允许侧栏拖到 `MIN_SIDE_WIDTH`，由 flex 挤压主列）。
const MIN_CHAT_RESERVE_PX: f64 = 240.0;
/// 工作区与任务均关闭时，右列仅保留工具栏（工作区/任务/状态/设置）的窄轨宽度。
const TOOLBAR_RAIL_WIDTH_PX: f64 = 84.0;
const AUTO_SCROLL_RESUME_GAP_PX: i32 = 24;

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn clamp_side_width_for_viewport(w: f64) -> f64 {
    let win = web_sys::window()
        .and_then(|win| win.inner_width().ok())
        .and_then(|v| v.as_f64())
        .unwrap_or(1200.0);
    let max_w = (win - MIN_CHAT_RESERVE_PX).clamp(MIN_SIDE_WIDTH, MAX_SIDE_WIDTH);
    w.clamp(MIN_SIDE_WIDTH, max_w)
}

fn load_f64_key(key: &str, default: f64) -> f64 {
    let Some(st) = local_storage() else {
        return clamp_side_width_for_viewport(default);
    };
    let Ok(Some(v)) = st.get_item(key) else {
        return clamp_side_width_for_viewport(default);
    };
    match v.parse::<f64>() {
        Ok(n) => clamp_side_width_for_viewport(n),
        _ => clamp_side_width_for_viewport(default),
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

/// 去掉摘要里**连续重复**的非空行（服务端或上游偶发会下发两行相同摘要，如 `read file: 2.md`）。
fn collapse_duplicate_summary_lines(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut last: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if last == Some(t) {
            continue;
        }
        last = Some(t);
        kept.push(t);
    }
    kept.join("\n")
}

fn tool_card_text(info: &ToolResultInfo) -> String {
    let sum = info.summary.as_deref().unwrap_or("").trim();
    let name = info.name.trim();
    if sum.is_empty() {
        return if !name.is_empty() {
            format!("工具：{name}")
        } else {
            "工具输出".to_string()
        };
    }
    let sum = collapse_duplicate_summary_lines(sum);
    if sum.is_empty() {
        return if !name.is_empty() {
            format!("工具：{name}")
        } else {
            "工具输出".to_string()
        };
    }
    // 首行 + 其余行；其余行中再剔除与首行相同的行，避免「标题行 + 正文重复首行」。
    let mut lines = sum.lines();
    let first = lines.next().unwrap_or_default().trim().to_string();
    if first.is_empty() {
        return if !name.is_empty() {
            format!("工具：{name}")
        } else {
            "工具输出".to_string()
        };
    }
    let rest: Vec<&str> = lines
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != first.as_str())
        .collect();
    if rest.is_empty() {
        return first;
    }
    let mut out = first;
    out.push_str("\n\n");
    out.push_str(&rest.join("\n"));
    out
}

fn format_agent_reply_plan_json_for_display(json_text: &str, goal: &str) -> Option<String> {
    let v: Value = serde_json::from_str(json_text).ok()?;
    let obj = v.as_object()?;
    if obj.get("type").and_then(|x| x.as_str()) != Some("agent_reply_plan") {
        return None;
    }
    let steps = obj.get("steps").and_then(|x| x.as_array())?;

    let mut lines = Vec::with_capacity(steps.len().saturating_add(1));
    let goal = goal.trim();
    if !goal.is_empty() {
        lines.push(goal.to_string());
    }
    if steps.is_empty() {
        if !goal.is_empty() {
            return Some(goal.to_string());
        }
        return Some("已生成分阶段规划。".to_string());
    }
    if !goal.is_empty() {
        lines.push(String::new());
    }
    for (idx, s) in steps.iter().enumerate() {
        let id = s
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .unwrap_or("step");
        let desc = s
            .get("description")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .unwrap_or("(未提供描述)");
        lines.push(format!("{}. `{}`: {}", idx + 1, id.trim(), desc.trim()));
    }
    Some(lines.join("\n"))
}

fn fenced_body_after_optional_jsonish_lang_label(inner: &str) -> Option<&str> {
    let s = inner.trim_start_matches(['\n', '\r', ' ', '\t']);
    if s.is_empty() {
        return Some("");
    }
    for label in ["json", "markdown", "md"] {
        if let Some(rest) = s.strip_prefix(label) {
            let mut chars = rest.chars();
            let next = chars.next();
            // 兼容两种形态：
            // 1) ```json\n{...}
            // 2) ```json{...}
            if next.is_none()
                || next == Some('\n')
                || next == Some('\r')
                || next == Some(' ')
                || next == Some('\t')
                || next == Some('{')
                || next == Some('[')
            {
                return Some(rest.trim_start_matches(['\n', '\r', ' ', '\t']));
            }
        }
    }
    None
}

fn triple_backtick_fence_count(s: &str) -> usize {
    s.match_indices("```").count()
}

fn first_fence_inner_looks_like_json_object(s: &str) -> bool {
    let mut it = s.split("```");
    let _ = it.next();
    let Some(inner) = it.next() else {
        return false;
    };
    let Some(body) = fenced_body_after_optional_jsonish_lang_label(inner) else {
        return false;
    };
    let b = body.trim();
    b.is_empty() || b.starts_with('{')
}

fn looks_like_incomplete_agent_reply_plan_whole_json(t: &str) -> bool {
    let t = t.trim();
    if !t.starts_with('{') {
        return false;
    }
    if t.contains("\"agent_reply_plan\"") {
        return true;
    }
    t.contains("\"type\"") && t.contains("\"version\"") && t.contains("\"steps\"")
}

fn should_buffer_agent_reply_plan_stream(stripped: &str) -> bool {
    if triple_backtick_fence_count(stripped) % 2 == 1
        && first_fence_inner_looks_like_json_object(stripped)
    {
        return true;
    }
    let t = stripped.trim();
    if !t.starts_with('{') {
        return false;
    }
    if format_agent_reply_plan_json_for_display(t, "").is_some() {
        return false;
    }
    serde_json::from_str::<Value>(t).is_err()
        && looks_like_incomplete_agent_reply_plan_whole_json(t)
}

fn prose_before_first_fence(s: &str) -> String {
    s.split("```").next().unwrap_or("").trim().to_string()
}

fn fence_inner_should_hide_agent_reply_plan_json(inner: &str) -> bool {
    let raw = inner.trim();
    let body = fenced_body_after_optional_jsonish_lang_label(raw)
        .unwrap_or(raw)
        .trim();
    if !body.starts_with('{') {
        return false;
    }
    if format_agent_reply_plan_json_for_display(body, "").is_some() {
        return true;
    }
    if !body.contains("\"agent_reply_plan\"") || !body.contains("\"steps\"") {
        return false;
    }
    serde_json::from_str::<Value>(body).is_ok()
}

fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
    let unclosed_trailing_fence = parts.len().is_multiple_of(2);
    let mut out = String::new();
    let mut i = 0usize;
    while i < parts.len() {
        out.push_str(parts[i]);
        i += 1;
        if i >= parts.len() {
            break;
        }
        let inner = parts[i];
        i += 1;
        if fence_inner_should_hide_agent_reply_plan_json(inner) {
            continue;
        }
        if unclosed_trailing_fence && i >= parts.len() && inner.trim().is_empty() {
            break;
        }
        out.push_str("```");
        out.push_str(inner);
        out.push_str("```");
    }
    out
}

pub(crate) fn assistant_text_for_display(raw: &str, is_streaming_last_assistant: bool) -> String {
    let trimmed = raw.trim();

    if is_streaming_last_assistant && should_buffer_agent_reply_plan_stream(trimmed) {
        return prose_before_first_fence(trimmed);
    }

    if let Some(display) = format_agent_reply_plan_json_for_display(trimmed, "")
        && !display.trim().is_empty()
    {
        return display;
    }

    // 无围栏但以前缀 JSON 输出规划：去掉前缀规划对象，保留后续终答正文。
    let t = raw.trim_start();
    if t.starts_with('{') && t.contains("\"agent_reply_plan\"") {
        let mut de = serde_json::Deserializer::from_str(t).into_iter::<Value>();
        if let Some(Ok(v)) = de.next()
            && v.as_object()
                .and_then(|o| o.get("type"))
                .and_then(|x| x.as_str())
                == Some("agent_reply_plan")
        {
            let offset = de.byte_offset();
            if offset < t.len() {
                let tail = t[offset..].trim();
                if !tail.is_empty() {
                    return tail.to_string();
                }
            }
        }
    }

    // 再做一次全量围栏剥离兜底：无论 `agent_reply_plan` 围栏出现在第几个代码块，都不回显原始 JSON。
    let stripped_fences = strip_agent_reply_plan_fence_blocks_for_display(raw);
    let stripped_trim = stripped_fences.trim();
    if stripped_trim != trimmed {
        if stripped_trim.is_empty() && raw.contains("\"agent_reply_plan\"") {
            return "已生成分阶段规划。".to_string();
        }
        return stripped_trim.to_string();
    }

    raw.to_string()
}

fn message_text_for_display(m: &StoredMessage) -> String {
    if m.role == "assistant" {
        let is_streaming_last_assistant = m.state.as_deref() == Some("loading");
        assistant_text_for_display(&m.text, is_streaming_last_assistant)
    } else {
        m.text.clone()
    }
}

fn message_created_ms() -> i64 {
    js_sys::Date::now() as i64
}

fn format_msg_time_label(ms: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    let h = d.get_hours();
    let m = d.get_minutes();
    Some(format!("{h:02}:{m:02}"))
}

fn message_role_label(m: &StoredMessage) -> &'static str {
    // 工具结果气泡用 `msg-tool` 样式区分，不再显示「工具」字样。
    if m.is_tool {
        return "";
    }
    match m.role.as_str() {
        "user" => "用户",
        "assistant" => "助手",
        "system" => "系统",
        _ => "其它",
    }
}

fn approval_session_id() -> String {
    format!(
        "approval_{}_{}",
        js_sys::Date::now() as i64,
        (js_sys::Math::random() * 1e9) as i64
    )
}

/// 首条用户消息生成侧栏/「管理会话」列表标题：压平换行、折叠空白，截断过长前缀。
fn title_from_user_prompt(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return DEFAULT_CHAT_SESSION_TITLE.to_string();
    }
    let single_line: String = t
        .chars()
        .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })
        .collect();
    let collapsed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 48;
    let n = collapsed.chars().count();
    if n <= MAX_CHARS {
        collapsed
    } else {
        format!(
            "{}…",
            collapsed
                .chars()
                .take(MAX_CHARS.saturating_sub(1))
                .collect::<String>()
        )
    }
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

fn export_session_json_for_id(sessions: RwSignal<Vec<ChatSession>>, id: &str) {
    let session = sessions.with(|list| list.iter().find(|s| s.id == id).cloned());
    let Some(s) = session else {
        return;
    };
    let file = session_to_export_file(&s);
    let Ok(json) = serde_json::to_string_pretty(&file) else {
        return;
    };
    let stem = export_filename_stem("chat_export");
    let name = format!("{stem}.json");
    if let Err(e) = trigger_download(&name, "application/json", &json) {
        if let Some(w) = web_sys::window() {
            let _ = w.alert_with_message(&e);
        }
    }
}

fn export_session_markdown_for_id(sessions: RwSignal<Vec<ChatSession>>, id: &str) {
    let session = sessions.with(|list| list.iter().find(|s| s.id == id).cloned());
    let Some(s) = session else {
        return;
    };
    let md = session_to_markdown(&s);
    let stem = export_filename_stem("chat_export");
    let name = format!("{stem}.md");
    if let Err(e) = trigger_download(&name, "text/markdown;charset=utf-8", &md) {
        if let Some(w) = web_sys::window() {
            let _ = w.alert_with_message(&e);
        }
    }
}

fn delete_session_after_confirm(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    id: &str,
) {
    let Some(w) = web_sys::window() else {
        return;
    };
    if !w
        .confirm_with_message("确定删除此本地会话？此操作不可恢复。")
        .unwrap_or(false)
    {
        return;
    }
    let id = id.to_string();
    let was_active = active_id.get() == id;
    sessions.update(|list| {
        list.retain(|s| s.id != id);
    });
    if sessions.with(|l| l.is_empty()) {
        let (list, def_id) = ensure_at_least_one(Vec::new());
        sessions.set(list);
        active_id.set(def_id.clone());
        draft.set(
            sessions
                .with(|l| l.iter().find(|s| s.id == def_id).map(|s| s.draft.clone()))
                .unwrap_or_default(),
        );
        conversation_id.set(None);
        return;
    }
    if was_active {
        let pick = sessions.with(|list| list[0].id.clone());
        active_id.set(pick.clone());
        draft.set(
            sessions
                .with(|l| l.iter().find(|s| s.id == pick).map(|s| s.draft.clone()))
                .unwrap_or_default(),
        );
        conversation_id.set(None);
    }
}

/// 左栏会话右键菜单锚点（`position: fixed` 使用视口坐标）。
#[derive(Clone)]
struct SessionContextAnchor {
    session_id: String,
    x: f64,
    y: f64,
}

fn clamp_session_ctx_menu_pos(cx: i32, cy: i32) -> (f64, f64) {
    const MENU_W: f64 = 190.0;
    const MENU_H: f64 = 148.0;
    let (ww, wh) = web_sys::window()
        .map(|w| {
            (
                w.inner_width()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(800.0),
                w.inner_height()
                    .ok()
                    .and_then(|v| v.as_f64())
                    .unwrap_or(600.0),
            )
        })
        .unwrap_or((800.0, 600.0));
    let x = (f64::from(cx)).clamp(6.0, (ww - MENU_W - 6.0).max(6.0));
    let y = (f64::from(cy)).clamp(6.0, (wh - MENU_H - 6.0).max(6.0));
    (x, y)
}

#[component]
fn SessionModalRow(
    id: String,
    title: String,
    message_count: usize,
    active: bool,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    conversation_id: RwSignal<Option<String>>,
    session_modal: RwSignal<bool>,
) -> impl IntoView {
    let id_rename = id.clone();
    let id_json = id.clone();
    let id_md = id.clone();
    let id_del = id.clone();
    let row_class = if active {
        "session-row active"
    } else {
        "session-row"
    };
    view! {
        <div class=row_class>
            <button
                type="button"
                class="session-open"
                on:click={
                    let id = id.clone();
                    move |_| {
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
                        session_modal.set(false);
                    }
                }
            >
                <span class="session-title">{title}</span>
                <span class="session-meta">{message_count}" 条"</span>
            </button>
            <div class="session-row-actions">
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    title="重命名"
                    on:click={
                        let sessions = sessions;
                        let id = id_rename.clone();
                        move |_| {
                            let default_title = sessions.with(|list| {
                                list.iter()
                                    .find(|s| s.id == id)
                                    .map(|s| s.title.clone())
                                    .unwrap_or_default()
                            });
                            let Some(w) = web_sys::window() else {
                                return;
                            };
                            let raw = match w.prompt_with_message_and_default("会话标题", &default_title)
                            {
                                Ok(Some(s)) => s,
                                Ok(None) | Err(_) => return,
                            };
                            let t = raw.trim().to_string();
                            if t.is_empty() {
                                return;
                            }
                            sessions.update(|list| {
                                if let Some(s) = list.iter_mut().find(|s| s.id == id) {
                                    s.title = t;
                                    s.updated_at = js_sys::Date::now() as i64;
                                }
                            });
                        }
                    }
                >
                    "重命名"
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    title="导出 JSON（ChatSessionFile v1）"
                    on:click={
                        let sessions = sessions;
                        let id = id_json.clone();
                        move |_| export_session_json_for_id(sessions, &id)
                    }
                >
                    "JSON"
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    title="导出 Markdown"
                    on:click={
                        let sessions = sessions;
                        let id = id_md.clone();
                        move |_| export_session_markdown_for_id(sessions, &id)
                    }
                >
                    "MD"
                </button>
                <button
                    type="button"
                    class="btn btn-danger btn-sm"
                    title="删除此会话"
                    on:click={
                        let sessions = sessions;
                        let active_id = active_id;
                        let draft = draft;
                        let conversation_id = conversation_id;
                        let id = id_del.clone();
                        move |_| {
                            delete_session_after_confirm(
                                sessions,
                                active_id,
                                draft,
                                conversation_id,
                                &id,
                            );
                        }
                    }
                >
                    "删除"
                </button>
            </div>
        </div>
    }
}

async fn reload_workspace_panel(
    workspace_loading: RwSignal<bool>,
    workspace_err: RwSignal<Option<String>>,
    workspace_path_draft: RwSignal<String>,
    workspace_data: RwSignal<Option<WorkspaceData>>,
) {
    workspace_loading.set(true);
    match fetch_workspace(None).await {
        Ok(d) => {
            workspace_err.set(None);
            workspace_path_draft.set(d.path.clone());
            workspace_data.set(Some(d));
        }
        Err(e) => {
            workspace_err.set(Some(e));
            workspace_data.set(None);
        }
    }
    workspace_loading.set(false);
}

fn begin_side_column_resize(
    ev: web_sys::MouseEvent,
    workspace_visible: RwSignal<bool>,
    tasks_visible: RwSignal<bool>,
    side_width: RwSignal<f64>,
    side_resize_dragging: RwSignal<bool>,
    side_resize_session: Rc<RefCell<Option<(f64, f64)>>>,
    side_resize_handles: Rc<RefCell<Option<(WindowListenerHandle, WindowListenerHandle)>>>,
) {
    if ev.button() != 0 {
        return;
    }
    if !workspace_visible.get_untracked() && !tasks_visible.get_untracked() {
        return;
    }
    ev.prevent_default();
    if let Some((m, u)) = side_resize_handles.borrow_mut().take() {
        m.remove();
        u.remove();
        *side_resize_session.borrow_mut() = None;
        side_resize_dragging.set(false);
    }

    *side_resize_session.borrow_mut() = Some((ev.client_x() as f64, side_width.get_untracked()));
    side_resize_dragging.set(true);

    let session_m = Rc::clone(&side_resize_session);
    let session_u = Rc::clone(&side_resize_session);
    let handles_slot = Rc::clone(&side_resize_handles);
    let side_w = side_width;
    let drag_sig = side_resize_dragging;

    let hm = window_event_listener(leptos::ev::mousemove, move |e: web_sys::MouseEvent| {
        let borrow = session_m.borrow();
        let Some((sx, sw)) = *borrow else {
            return;
        };
        let cx = e.client_x() as f64;
        side_w.set(clamp_side_width_for_viewport(sw - (cx - sx)));
    });

    let hu = window_event_listener(leptos::ev::mouseup, move |_e: web_sys::MouseEvent| {
        *session_u.borrow_mut() = None;
        drag_sig.set(false);
        if let Some((m, u)) = handles_slot.borrow_mut().take() {
            m.remove();
            u.remove();
        }
    });

    *side_resize_handles.borrow_mut() = Some((hm, hu));
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
        if workspace_visible.get() && initialized.get() {
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
                let stream_result = send_chat_stream(
                    text,
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
                <div class="nav-rail-scroll">
                    <div class="nav-rail-scroll-label">"最近"</div>
                    {move || {
                        let mut v: Vec<ChatSession> = sessions.get().into_iter().collect();
                        v.sort_by_key(|s| std::cmp::Reverse(s.updated_at));
                        v.into_iter()
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
                                                mobile_nav_open.set(false);
                                            }
                                        }
                                    >
                                        <span class="nav-session-title">{title}</span>
                                        <span class="nav-session-meta">{n}" 条"</span>
                                    </button>
                                }
                            })
                            .collect_view()
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

            <div
                class:main-row-resizing=move || side_resize_dragging.get()
                class="main-row"
            >
                <div class="chat-column">
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
                                                        <li>"最右列为工具栏与工作区/任务面板：右列顶部可开关工作区、任务、状态栏，并通过「设置」调整主题与页面背景。"</li>
                                                    </ul>
                                                </div>
                                            </div>
                                        }
                                        .into_any()
                                    } else {
                                        msgs
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
                                                let role_lbl = message_role_label(&m);
                                                let time_str =
                                                    format_msg_time_label(m.created_at).unwrap_or_default();
                                                view! {
                                                    <div class=class_final>
                                                        <div class="msg-meta" aria-hidden="true">
                                                            <span class="msg-meta-role">{role_lbl}</span>
                                                            <span class="msg-meta-time">{time_str}</span>
                                                        </div>
                                                        <span class="msg-body">{message_text_for_display(&m)}</span>
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
                        !workspace_visible.get() && !tasks_visible.get()
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
                                workspace_visible,
                                tasks_visible,
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
                        if !workspace_visible.get() && !tasks_visible.get() {
                            c.push_str(" side-column-rail-only");
                        }
                        c
                    }
                    style:width=move || {
                        if workspace_visible.get() || tasks_visible.get() {
                            format!("{}px", side_width.get())
                        } else {
                            format!("{TOOLBAR_RAIL_WIDTH_PX}px")
                        }
                    }
                >
                        <div class="shell-main-toolbar" role="toolbar" aria-label="视图与设置">
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm"
                                class:active=move || workspace_visible.get()
                                on:click=move |_| workspace_visible.update(|v| *v = !*v)
                                title="工作区"
                            >
                                "工作区"
                            </button>
                            <button
                                type="button"
                                class="btn btn-secondary btn-sm"
                                class:active=move || tasks_visible.get()
                                on:click=move |_| tasks_visible.update(|v| *v = !*v)
                                title="任务"
                            >
                                "任务"
                            </button>
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
                                                status_data
                                                    .get()
                                                    .map(|d| d.model)
                                                    .unwrap_or_else(|| "-".to_string())
                                            }}</span>
                                        </span>
                                        <span class="status-chip status-chip-url" title=move || {
                                            status_data
                                                .get()
                                                .map(|d| d.api_base)
                                                .unwrap_or_else(|| "-".to_string())
                                        }>
                                            <span class="status-chip-label">"base_url"</span>
                                            <span class="status-chip-value">{move || {
                                                status_data
                                                    .get()
                                                    .map(|d| d.api_base)
                                                    .unwrap_or_else(|| "-".to_string())
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
                            <span class="modal-badge">"外观"</span>
                            <span class="modal-head-spacer"></span>
                            <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| settings_modal.set(false)>
                                "关闭"
                            </button>
                        </div>
                        <div class="modal-body">
                            <p class="modal-hint">"主题与页面背景选项保存在本机浏览器（localStorage）。"</p>
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

#[cfg(test)]
mod tests {
    use super::assistant_text_for_display;
    use super::title_from_user_prompt;
    use crate::storage::DEFAULT_CHAT_SESSION_TITLE;

    #[test]
    fn title_from_prompt_flattens_whitespace() {
        assert_eq!(title_from_user_prompt("  hello\nworld  "), "hello world");
    }

    #[test]
    fn title_from_prompt_truncates_long() {
        let body = "a".repeat(60);
        let out = title_from_user_prompt(&body);
        assert!(out.ends_with('…'), "got {out:?}");
        assert!(out.chars().count() <= 48, "len {}", out.chars().count());
    }

    #[test]
    fn title_from_blank_is_default() {
        assert_eq!(
            title_from_user_prompt("  \n\t  "),
            DEFAULT_CHAT_SESSION_TITLE
        );
    }

    #[test]
    fn hide_inline_agent_reply_plan_json_fence() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```"#;
        let out = assistant_text_for_display(raw, true);
        assert!(
            !out.contains("agent_reply_plan"),
            "raw agent_reply_plan json should be filtered: {out}"
        );
        assert!(
            !out.contains("```"),
            "agent_reply_plan fence should be stripped: {out}"
        );
    }

    #[test]
    fn no_task_empty_plan_has_non_empty_fallback() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false);
        assert!(
            !out.trim().is_empty(),
            "filtered plan text should not become empty"
        );
    }

    #[test]
    fn keep_answer_after_fenced_plan_json() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```最终结论：已完成。"#;
        let out = assistant_text_for_display(raw, false);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn keep_answer_after_unfenced_plan_json_prefix() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}最终结论：继续执行。"#;
        let out = assistant_text_for_display(raw, false);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }
}
