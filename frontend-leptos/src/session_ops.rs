//! 会话列表、导出、删除与消息元数据辅助逻辑。

use leptos::prelude::*;

use crate::session_export::{
    export_filename_stem, session_to_export_file, session_to_markdown, trigger_download,
};
use crate::storage::{
    ChatSession, DEFAULT_CHAT_SESSION_TITLE, StoredMessage, ensure_at_least_one, make_session_id,
};

pub fn make_message_id() -> String {
    make_session_id()
}

/// 去掉失败助手泡及其后消息，挂上新的 loading 助手泡；返回本回合用户原文与新助手 id。
pub fn prepare_retry_failed_assistant_turn(
    sessions: &mut [ChatSession],
    active_id: &str,
    failed_asst_id: &str,
) -> Option<(String, String)> {
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
    s.messages.truncate(idx);
    let new_asst_id = make_message_id();
    let now = message_created_ms();
    s.messages.push(StoredMessage {
        id: new_asst_id.clone(),
        role: "assistant".to_string(),
        text: String::new(),
        state: Some("loading".to_string()),
        is_tool: false,
        created_at: now,
    });
    Some((user_text, new_asst_id))
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

pub fn message_role_label(m: &StoredMessage) -> &'static str {
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

pub fn export_session_json_for_id(sessions: RwSignal<Vec<ChatSession>>, id: &str) {
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

pub fn export_session_markdown_for_id(sessions: RwSignal<Vec<ChatSession>>, id: &str) {
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

pub fn delete_session_after_confirm(
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
pub struct SessionContextAnchor {
    pub session_id: String,
    pub x: f64,
    pub y: f64,
}

pub fn clamp_session_ctx_menu_pos(cx: i32, cy: i32) -> (f64, f64) {
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
