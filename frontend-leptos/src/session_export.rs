//! 浏览器内导出会话：JSON 与 `runtime/chat_export::ChatSessionFile` v1 对齐；Markdown 与 CLI `messages_to_markdown` 语义对齐（跳过纯 system，工具卡为「工具」段）。

use gloo_timers::callback::Timeout;
use serde::Serialize;
use wasm_bindgen::JsCast;

use crate::message_format::assistant_text_for_display;
use crate::storage::{ChatSession, StoredMessage};

/// 须与 `src/runtime/chat_export.rs` 中 `CHAT_SESSION_FILE_VERSION` 一致。
pub const CHAT_SESSION_FILE_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
pub struct ExportMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatSessionFile {
    pub version: u32,
    pub messages: Vec<ExportMessage>,
}

pub fn session_to_export_file(session: &ChatSession) -> ChatSessionFile {
    ChatSessionFile {
        version: CHAT_SESSION_FILE_VERSION,
        messages: stored_messages_to_export(&session.messages),
    }
}

fn stored_messages_to_export(messages: &[StoredMessage]) -> Vec<ExportMessage> {
    let mut out = Vec::new();
    for m in messages {
        if m.role == "system" && m.is_tool {
            out.push(ExportMessage {
                role: "tool".to_string(),
                content: Some(message_text_for_export(m)),
                name: Some("tool".to_string()),
            });
            continue;
        }
        if m.role == "system" {
            continue;
        }
        out.push(ExportMessage {
            role: m.role.clone(),
            content: Some(message_text_for_export(m)),
            name: None,
        });
    }
    out
}

fn message_text_for_export(m: &StoredMessage) -> String {
    if m.role == "assistant" {
        assistant_text_for_display(&m.text, m.state.as_deref() == Some("loading"))
    } else {
        m.text.clone()
    }
}

fn markdown_sections_for_export(messages: &[ExportMessage]) -> String {
    let mut md = String::new();
    for m in messages {
        let role = m.role.as_str();
        if role == "system" {
            continue;
        }
        let heading = match role {
            "user" => "## 用户",
            "assistant" => "## 助手",
            "tool" => "## 工具",
            _ => "## 其它",
        };
        md.push_str(heading);
        md.push_str("\n\n");
        md.push_str(m.content.as_deref().unwrap_or(""));
        md.push_str("\n\n");
    }
    md
}

/// 与 `chat_export::messages_to_markdown` 一致：跳过 `system`；`tool` 与 `user`/`assistant` 分段。
pub fn session_to_markdown(session: &ChatSession) -> String {
    let messages = stored_messages_to_export(&session.messages);
    let mut md = String::from("# CrabMate 聊天记录\n\n");
    md.push_str(&markdown_sections_for_export(&messages));
    md
}

/// 按会话内顺序导出**已选 id** 对应的消息（与全会话 Markdown 规则相同；未选中的 id 忽略）。
pub fn stored_messages_by_ids_to_markdown(
    all_messages: &[StoredMessage],
    selected_ids: &[String],
) -> String {
    use std::collections::HashSet;

    let set: HashSet<&str> = selected_ids.iter().map(|s| s.as_str()).collect();
    let subset: Vec<StoredMessage> = all_messages
        .iter()
        .filter(|m| set.contains(m.id.as_str()))
        .cloned()
        .collect();
    let messages = stored_messages_to_export(&subset);
    let mut md = String::from("# CrabMate 聊天记录（已选消息）\n\n");
    md.push_str(&markdown_sections_for_export(&messages));
    md
}

pub fn export_filename_stem(prefix: &str) -> String {
    let now = js_sys::Date::new_0();
    let y = now.get_full_year() as i32;
    let mo = now.get_month() + 1;
    let d = now.get_date();
    let h = now.get_hours();
    let mi = now.get_minutes();
    let s = now.get_seconds();
    format!(
        "{}_{:04}{:02}{:02}_{:02}{:02}{:02}",
        prefix, y, mo, d, h, mi, s
    )
}

/// 触发浏览器下载 UTF-8 文本；失败时返回说明字符串。
pub fn trigger_download(filename: &str, mime: &str, body: &str) -> Result<(), String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let document = window.document().ok_or_else(|| "no document".to_string())?;

    let parts = js_sys::Array::new();
    parts.push(&wasm_bindgen::JsValue::from_str(body));
    let opts = web_sys::BlobPropertyBag::new();
    opts.set_type(mime);
    let blob = web_sys::Blob::new_with_str_sequence_and_options(&parts, &opts)
        .map_err(|e| format!("Blob: {:?}", e))?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)
        .map_err(|e| format!("object URL: {:?}", e))?;

    let a = document
        .create_element("a")
        .map_err(|e| format!("create a: {:?}", e))?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| "a element".to_string())?;
    a.set_href(&url);
    a.set_download(filename);
    a.set_attribute("rel", "noopener")
        .map_err(|e| format!("rel: {:?}", e))?;
    a.style().set_property("display", "none").ok();
    let body_el = document.body().ok_or_else(|| "no body".to_string())?;
    body_el
        .append_child(&a)
        .map_err(|e| format!("append: {:?}", e))?;
    a.click();
    body_el.remove_child(&a).ok();

    let url_clone = url.clone();
    Timeout::new(0, move || {
        let _ = web_sys::Url::revoke_object_url(&url_clone);
    })
    .forget();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessage;

    fn msg(id: &str, role: &str, text: &str, is_tool: bool) -> StoredMessage {
        StoredMessage {
            id: id.to_string(),
            role: role.to_string(),
            text: text.to_string(),
            state: None,
            is_tool,
            created_at: 0,
        }
    }

    #[test]
    fn by_ids_keeps_session_order_and_omits_unselected() {
        let session = ChatSession {
            id: "s1".to_string(),
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![
                msg("a", "user", "first", false),
                msg("b", "assistant", "second", false),
                msg("c", "user", "third", false),
            ],
            updated_at: 0,
        };
        let md = stored_messages_by_ids_to_markdown(&session.messages, &["c".into(), "a".into()]);
        assert!(md.contains("first"));
        assert!(!md.contains("second"));
        assert!(md.contains("third"));
        let pos_first = md.find("first").unwrap();
        let pos_third = md.find("third").unwrap();
        assert!(
            pos_first < pos_third,
            "export should follow session order, not selection order"
        );
    }

    #[test]
    fn skips_plain_system_keeps_tool_cards_as_tool_role() {
        let session = ChatSession {
            id: "s1".to_string(),
            title: "t".to_string(),
            draft: String::new(),
            messages: vec![
                msg("1", "user", "hi", false),
                msg("2", "system", "hidden", false),
                msg("3", "system", "tool out", true),
                msg("4", "assistant", "ok", false),
            ],
            updated_at: 0,
        };
        let file = session_to_export_file(&session);
        assert_eq!(file.messages.len(), 3);
        assert_eq!(file.messages[0].role, "user");
        assert_eq!(file.messages[1].role, "tool");
        assert_eq!(file.messages[1].name.as_deref(), Some("tool"));
        assert_eq!(file.messages[2].role, "assistant");
    }
}
