//! 将 `GET /conversation/messages` 返回的 OpenAI 兼容消息转为 [`crate::storage::StoredMessage`]。

use serde::Deserialize;
use serde_json::Value;

use crate::storage::StoredMessage;

use crate::conversation_hydrate_timeline::{
    append_assistant_tool_calls_timeline_card, append_crabmate_timeline_system_message,
    append_tool_role_timeline_row,
};

/// 与后端 `src/types.rs` 中 `CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME` 一致。
const CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME: &str = "crabmate_first_turn_workspace_context";

#[derive(Debug, Deserialize)]
pub struct ConversationMessagesResponse {
    #[allow(dead_code)]
    pub conversation_id: String,
    pub revision: u64,
    #[serde(default)]
    pub active_agent_role: Option<String>,
    pub messages: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct ApiMessage {
    role: String,
    content: Option<Value>,
    #[serde(default, alias = "reasoning")]
    reasoning_content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Value>,
    #[serde(default)]
    name: Option<String>,
    #[allow(dead_code)]
    #[serde(default)]
    tool_call_id: Option<String>,
}

fn text_from_content(content: &Option<Value>) -> (String, Vec<String>) {
    let Some(c) = content else {
        return (String::new(), Vec::new());
    };
    match c {
        Value::String(s) => (s.clone(), Vec::new()),
        Value::Array(parts) => {
            let mut text = String::new();
            let mut urls = Vec::new();
            for p in parts {
                let Some(obj) = p.as_object() else {
                    continue;
                };
                let ty = obj.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if ty == "text" {
                    if let Some(t) = obj.get("text").and_then(|x| x.as_str()) {
                        if !text.is_empty() {
                            text.push('\n');
                        }
                        text.push_str(t);
                    }
                } else if ty == "image_url" {
                    if let Some(u) = obj
                        .get("image_url")
                        .and_then(|iu| iu.get("url"))
                        .and_then(|x| x.as_str())
                    {
                        let u = u.trim();
                        if !u.is_empty() {
                            urls.push(u.to_string());
                        }
                    }
                }
            }
            (text, urls)
        }
        _ => (String::new(), Vec::new()),
    }
}

/// 返回 `true` 表示该条已由分支消费（外层应 `continue`）。
fn hydrate_try_special_cases(
    parsed: &ApiMessage,
    role: &str,
    name: &str,
    text: &str,
    reasoning: &str,
    base_ms: i64,
    out: &mut Vec<StoredMessage>,
    t: &mut i64,
) -> bool {
    if role == "system" && name == "crabmate_ui_sep" {
        return true;
    }
    if role == "user" && name == CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME {
        return true;
    }
    if role == "system" && name == "crabmate_timeline" {
        append_crabmate_timeline_system_message(text, base_ms, out, t);
        return true;
    }
    if role == "assistant"
        && text.trim().is_empty()
        && reasoning.trim().is_empty()
        && let Some(ref tc) = parsed.tool_calls
    {
        append_assistant_tool_calls_timeline_card(tc, base_ms, out, t);
        return true;
    }
    if role == "tool" {
        append_tool_role_timeline_row(name, text, base_ms, out, t);
        return true;
    }
    false
}

/// 将会话快照转为 UI 消息列表（新 id；`created_at` 从 `base_ms` 递增以保证顺序）。
pub fn stored_messages_from_conversation_api_with_base(
    msgs: &[Value],
    base_ms: i64,
) -> Vec<StoredMessage> {
    let mut out: Vec<StoredMessage> = Vec::new();
    let mut t = base_ms;
    for raw in msgs {
        let parsed: ApiMessage = match serde_json::from_value(raw.clone()) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let role = parsed.role.trim().to_string();
        if role.is_empty() {
            continue;
        }
        let (text, image_urls) = text_from_content(&parsed.content);
        let reasoning = parsed.reasoning_content.clone().unwrap_or_default();
        let name = parsed.name.as_deref().unwrap_or("").trim();

        if hydrate_try_special_cases(
            &parsed, &role, name, &text, &reasoning, base_ms, &mut out, &mut t,
        ) {
            continue;
        }

        let id = format!("h_{}_{}", base_ms, out.len());
        t = t.saturating_add(1);
        let is_user = role == "user";
        out.push(StoredMessage {
            id,
            role,
            text,
            reasoning_text: reasoning,
            image_urls: if is_user { image_urls } else { vec![] },
            state: None,
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: t,
        });
    }
    out
}

/// WASM 入口：`base_ms` 为当前时间。
pub fn stored_messages_from_conversation_api(msgs: &[Value]) -> Vec<StoredMessage> {
    stored_messages_from_conversation_api_with_base(msgs, js_sys::Date::now() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_user_text_and_skips_ui_sep() {
        let msgs = vec![
            json!({"role":"system","name":"crabmate_ui_sep","content":"x"}),
            json!({"role":"user","content":"hi"}),
        ];
        let out = stored_messages_from_conversation_api_with_base(&msgs, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[0].text, "hi");
    }

    #[test]
    fn skips_first_turn_workspace_context_user() {
        let msgs = vec![
            json!({"role":"user","name":CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME,"content":"profile"}),
            json!({"role":"user","content":"real"}),
        ];
        let out = stored_messages_from_conversation_api_with_base(&msgs, 0);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "real");
    }
}
