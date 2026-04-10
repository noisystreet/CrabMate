//! 会话列表、导出、删除与消息元数据辅助逻辑。

use leptos::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{HtmlElement, Node};

use crate::session_export::{
    export_filename_stem, session_to_export_file, session_to_markdown, trigger_download,
};
use crate::storage::{
    ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, ensure_at_least_one, make_session_id,
};

pub fn make_message_id() -> String {
    make_session_id()
}

/// 本地估算：当前会话所有消息正文 Unicode 标量个数 + 输入框草稿（与服务端 `context_char_budget` 对照用，非精确 token）。
pub fn estimate_context_chars_for_active_session(
    sessions: &[ChatSession],
    active_id: &str,
    composer_draft: &str,
) -> usize {
    let from_msgs: usize = sessions
        .iter()
        .find(|s| s.id == active_id)
        .map(|s| {
            s.messages
                .iter()
                .map(|m| m.text.chars().count())
                .sum::<usize>()
        })
        .unwrap_or(0);
    from_msgs.saturating_add(composer_draft.chars().count())
}

/// 去掉失败助手泡及其后消息，挂上新的 loading 助手泡；返回本回合用户原文与新助手 id。
/// 第 `idx` 条消息若为普通用户消息，返回其在本会话中的 **0-based 用户序号**（与 `POST /chat/branch` 的 `before_user_ordinal` 一致）。
pub fn user_ordinal_for_message_index(messages: &[StoredMessage], idx: usize) -> Option<u64> {
    let m = messages.get(idx)?;
    if m.role != "user" || m.is_tool {
        return None;
    }
    let mut ord = 0_u64;
    for (i, x) in messages.iter().enumerate() {
        if i >= idx {
            break;
        }
        if x.role == "user" && !x.is_tool {
            ord = ord.saturating_add(1);
        }
    }
    Some(ord)
}

/// 在 `msg_index` 之前（不含本条）最近一条 **普通用户** 消息（`role == user` 且非工具卡）的 id。
///
/// 用于工具结果气泡「↑」跳转到本回合对应的用户提问；若无则返回 `None`。
pub fn preceding_plain_user_message_id(
    messages: &[StoredMessage],
    msg_index: usize,
) -> Option<String> {
    let mut j = msg_index;
    while j > 0 {
        j -= 1;
        let m = messages.get(j)?;
        if m.role == "user" && !m.is_tool {
            return Some(m.id.clone());
        }
    }
    None
}

/// 保留指定用户气泡（同 id），删除其后的消息，并挂上新的 loading 助手泡；返回用户原文与新助手 id。
pub fn truncate_at_user_message_and_prepare_regenerate(
    sessions: &mut [ChatSession],
    active_id: &str,
    user_msg_id: &str,
) -> Option<(String, Vec<String>, String)> {
    let s = sessions.iter_mut().find(|sess| sess.id == active_id)?;
    let idx = s.messages.iter().position(|m| m.id == user_msg_id)?;
    let um = s.messages.get(idx)?;
    if um.role != "user" || um.is_tool {
        return None;
    }
    let user_msg = um.clone();
    let user_text = user_msg.text.clone();
    let user_images = user_msg.image_urls.clone();
    s.messages.truncate(idx);
    s.messages.push(user_msg);
    let new_asst_id = make_message_id();
    let now = message_created_ms();
    s.messages.push(StoredMessage {
        id: new_asst_id.clone(),
        role: "assistant".to_string(),
        text: String::new(),
        image_urls: vec![],
        state: Some("loading".to_string()),
        is_tool: false,
        created_at: now,
    });
    Some((user_text, user_images, new_asst_id))
}

/// 截断到指定用户消息之前（含该条及之后全部移除），不追加助手泡。
pub fn truncate_at_user_message_branch_local(
    sessions: &mut [ChatSession],
    active_id: &str,
    user_msg_id: &str,
) -> bool {
    let Some(s) = sessions.iter_mut().find(|sess| sess.id == active_id) else {
        return false;
    };
    let Some(idx) = s.messages.iter().position(|m| m.id == user_msg_id) else {
        return false;
    };
    let um = match s.messages.get(idx) {
        Some(m) => m,
        None => return false,
    };
    if um.role != "user" || um.is_tool {
        return false;
    }
    s.messages.truncate(idx);
    true
}

pub fn prepare_retry_failed_assistant_turn(
    sessions: &mut [ChatSession],
    active_id: &str,
    failed_asst_id: &str,
) -> Option<(String, Vec<String>, String)> {
    let s = sessions.iter_mut().find(|sess| sess.id == active_id)?;
    let idx = s.messages.iter().position(|m| {
        m.id == failed_asst_id
            && m.role == "assistant"
            && !m.is_tool
            && m.state.as_deref() == Some("error")
    })?;
    if idx == 0 {
        return None;
    }
    if s.messages[idx - 1].role != "user" {
        return None;
    }
    let user_text = s.messages[idx - 1].text.clone();
    let user_images = s.messages[idx - 1].image_urls.clone();
    s.messages.truncate(idx);
    let new_asst_id = make_message_id();
    let now = message_created_ms();
    s.messages.push(StoredMessage {
        id: new_asst_id.clone(),
        role: "assistant".to_string(),
        text: String::new(),
        image_urls: vec![],
        state: Some("loading".to_string()),
        is_tool: false,
        created_at: now,
    });
    Some((user_text, user_images, new_asst_id))
}

pub fn message_created_ms() -> i64 {
    js_sys::Date::now() as i64
}

pub fn format_msg_time_label(ms: i64) -> Option<String> {
    if ms <= 0 {
        return None;
    }
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    let h = d.get_hours();
    let m = d.get_minutes();
    Some(format!("{h:02}:{m:02}"))
}

pub fn message_role_label(m: &StoredMessage, locale: crate::i18n::Locale) -> &'static str {
    // 工具结果气泡用 `msg-tool` 样式区分，不再显示「工具」字样。
    if m.is_tool {
        return "";
    }
    match m.role.as_str() {
        "user" => crate::i18n::msg_role_user(locale),
        "assistant" => crate::i18n::msg_role_assistant(locale),
        "system" => crate::i18n::msg_role_system(locale),
        _ => crate::i18n::msg_role_other(locale),
    }
}

pub fn approval_session_id() -> String {
    format!(
        "approval_{}_{}",
        js_sys::Date::now() as i64,
        (js_sys::Math::random() * 1e9) as i64
    )
}

/// 首条用户消息生成侧栏/「管理会话」列表标题：压平换行、折叠空白，截断过长前缀。
pub fn title_from_user_prompt(text: &str) -> String {
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

pub fn patch_active_session(
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

/// 将输入框草稿写入指定会话（切换会话、新建会话前调用），触发 `sessions` 更新与本地持久化。
pub fn flush_composer_draft_to_session(
    sessions: RwSignal<Vec<ChatSession>>,
    session_id: &str,
    text: &str,
) {
    if session_id.is_empty() {
        return;
    }
    let t = text.to_string();
    patch_active_session(sessions, session_id, move |s| {
        s.draft = t;
    });
}

pub fn export_session_json_for_id(
    sessions: RwSignal<Vec<ChatSession>>,
    id: &str,
    loc: crate::i18n::Locale,
) {
    let session = sessions.with(|list| list.iter().find(|s| s.id == id).cloned());
    let Some(s) = session else {
        return;
    };
    let file = session_to_export_file(&s, loc);
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

fn element_for_closest(n: &Node) -> Option<web_sys::Element> {
    n.dyn_ref::<web_sys::Element>()
        .cloned()
        .or_else(|| n.parent_element())
}

/// 当前选区为消息列表内**同一** `.msg` 气泡中的非空文本时，返回可复制字符串；否则 `None`。
/// 用于聊天区自定义右键菜单中的「复制选中文字」。
pub fn selected_text_in_messages_for_context_copy(messages: &HtmlElement) -> Option<String> {
    let window = web_sys::window()?;
    let sel = window.get_selection().ok().flatten()?;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let anchor = sel.anchor_node()?;
    let focus = sel.focus_node()?;
    let messages_node: &Node = messages.unchecked_ref();
    if !messages_node.contains(Some(&anchor)) || !messages_node.contains(Some(&focus)) {
        return None;
    }
    let host_a = element_for_closest(&anchor)?;
    let host_b = element_for_closest(&focus)?;
    let msg_a = host_a.closest(".msg").ok().flatten()?;
    let msg_b = host_b.closest(".msg").ok().flatten()?;
    let na: &Node = msg_a.unchecked_ref();
    let nb: &Node = msg_b.unchecked_ref();
    if !na.is_same_node(Some(nb)) {
        return None;
    }
    let range = sel.get_range_at(0).ok()?;
    let frag = range.clone_contents().ok()?;
    let text = frag.text_content()?;
    let t = text.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 将文本写入系统剪贴板；失败时 `window.alert` 简短提示。
pub fn write_clipboard_text(text: &str, locale: crate::i18n::Locale) {
    let Some(w) = web_sys::window() else {
        return;
    };
    let t = text.to_string();
    let msg = crate::i18n::clipboard_failed(locale).to_string();
    wasm_bindgen_futures::spawn_local(async move {
        let nav = w.navigator();
        let clip = nav.clipboard();
        match JsFuture::from(clip.write_text(&t)).await {
            Ok(_) => {}
            Err(_) => {
                let _ = w.alert_with_message(&msg);
            }
        }
    });
}

pub fn export_session_markdown_for_id(
    sessions: RwSignal<Vec<ChatSession>>,
    id: &str,
    loc: crate::i18n::Locale,
) {
    let session = sessions.with(|list| list.iter().find(|s| s.id == id).cloned());
    let Some(s) = session else {
        return;
    };
    let md = session_to_markdown(&s, loc);
    let stem = export_filename_stem("chat_export");
    let name = format!("{stem}.md");
    if let Err(e) = trigger_download(&name, "text/markdown;charset=utf-8", &md) {
        if let Some(w) = web_sys::window() {
            let _ = w.alert_with_message(&e);
        }
    }
}

pub fn delete_session_after_confirm(
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    draft: RwSignal<String>,
    session_sync: RwSignal<crate::session_sync::SessionSyncState>,
    id: &str,
    locale: crate::i18n::Locale,
) {
    let Some(w) = web_sys::window() else {
        return;
    };
    let confirm_msg = crate::i18n::delete_session_confirm(locale);
    if !w.confirm_with_message(confirm_msg).unwrap_or(false) {
        return;
    }
    let id = id.to_string();
    let was_active = active_id.get() == id;
    sessions.update(|list| {
        list.retain(|s| s.id != id);
    });
    if sessions.with(|l| l.is_empty()) {
        let (list, def_id) = ensure_at_least_one(
            Vec::new(),
            crate::i18n::default_session_title(locale).to_string(),
        );
        sessions.set(list);
        active_id.set(def_id.clone());
        draft.set(
            sessions
                .with(|l| l.iter().find(|s| s.id == def_id).map(|s| s.draft.clone()))
                .unwrap_or_default(),
        );
        session_sync.set(crate::session_sync::SessionSyncState::local_only());
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
        session_sync.set(crate::session_sync::SessionSyncState::local_only());
    }
}

/// 左栏会话右键菜单锚点（`position: fixed` 使用视口坐标）。
#[derive(Clone)]
pub struct SessionContextAnchor {
    pub session_id: String,
    pub x: f64,
    pub y: f64,
}

#[cfg(test)]
mod message_branch_tests {
    use super::*;

    #[test]
    fn user_ordinal_matches_backend_semantics() {
        let messages = vec![
            StoredMessage {
                id: "1".into(),
                role: "user".into(),
                text: "a".into(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: 0,
            },
            StoredMessage {
                id: "2".into(),
                role: "assistant".into(),
                text: "b".into(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: 0,
            },
            StoredMessage {
                id: "3".into(),
                role: "user".into(),
                text: "c".into(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: 0,
            },
        ];
        assert_eq!(user_ordinal_for_message_index(&messages, 0), Some(0));
        assert_eq!(user_ordinal_for_message_index(&messages, 2), Some(1));
        assert!(user_ordinal_for_message_index(&messages, 1).is_none());
    }

    #[test]
    fn preceding_plain_user_finds_latest_user_before_tool() {
        let messages = vec![
            StoredMessage {
                id: "u0".into(),
                role: "user".into(),
                text: "first".into(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: 0,
            },
            StoredMessage {
                id: "a0".into(),
                role: "assistant".into(),
                text: "ok".into(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: 0,
            },
            StoredMessage {
                id: "t0".into(),
                role: "system".into(),
                text: "tool".into(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                created_at: 0,
            },
        ];
        assert_eq!(
            preceding_plain_user_message_id(&messages, 2).as_deref(),
            Some("u0")
        );
    }

    #[test]
    fn preceding_plain_user_skips_tool_user_role() {
        let messages = vec![
            StoredMessage {
                id: "u0".into(),
                role: "user".into(),
                text: "q".into(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: 0,
            },
            StoredMessage {
                id: "t0".into(),
                role: "user".into(),
                text: "tool card".into(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                created_at: 0,
            },
            StoredMessage {
                id: "t1".into(),
                role: "system".into(),
                text: "tool".into(),
                image_urls: vec![],
                state: None,
                is_tool: true,
                created_at: 0,
            },
        ];
        assert_eq!(
            preceding_plain_user_message_id(&messages, 2).as_deref(),
            Some("u0")
        );
    }

    #[test]
    fn preceding_plain_user_none_at_start() {
        let messages = vec![StoredMessage {
            id: "t0".into(),
            role: "system".into(),
            text: "tool".into(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            created_at: 0,
        }];
        assert!(preceding_plain_user_message_id(&messages, 0).is_none());
    }

    #[test]
    fn truncate_branch_local_drops_from_user_onwards() {
        let mut sessions = vec![ChatSession {
            id: "s1".into(),
            title: "t".into(),
            draft: String::new(),
            messages: vec![
                StoredMessage {
                    id: "u0".into(),
                    role: "user".into(),
                    text: "first".into(),
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
                    created_at: 0,
                },
                StoredMessage {
                    id: "a0".into(),
                    role: "assistant".into(),
                    text: "ok".into(),
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
                    created_at: 0,
                },
                StoredMessage {
                    id: "u1".into(),
                    role: "user".into(),
                    text: "retry me".into(),
                    image_urls: vec![],
                    state: None,
                    is_tool: false,
                    created_at: 0,
                },
            ],
            updated_at: 0,
        }];
        assert!(truncate_at_user_message_branch_local(
            &mut sessions,
            "s1",
            "u1"
        ));
        // 与后端一致：`before_user_ordinal`=1 时保留第 0 条用户及其后直到下一条用户之前（含中间助手）。
        assert_eq!(sessions[0].messages.len(), 2);
        assert_eq!(sessions[0].messages[0].id, "u0");
        assert_eq!(sessions[0].messages[1].id, "a0");
    }
}

pub fn clamp_session_ctx_menu_pos(cx: i32, cy: i32) -> (f64, f64) {
    const MENU_W: f64 = 190.0;
    // 上限略大，兼容聊天区多选菜单（多项）与侧栏会话菜单。
    const MENU_H: f64 = 220.0;
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

#[cfg(test)]
mod tests {
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
}
