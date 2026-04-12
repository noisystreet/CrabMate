//! 将 `GET /conversation/messages` 返回的 OpenAI 兼容消息转为 [`crate::storage::StoredMessage`]。

use serde::Deserialize;
use serde_json::Value;

use crate::message_format::staged_timeline_system_message_body;
use crate::storage::StoredMessage;
use crate::timeline_scan::{
    timeline_state_staged_end, timeline_state_staged_start, timeline_state_tool,
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

fn tool_calls_summary(tool_calls: &Value) -> String {
    let Some(arr) = tool_calls.as_array() else {
        return String::new();
    };
    let mut lines: Vec<String> = Vec::new();
    for tc in arr {
        let Some(obj) = tc.as_object() else {
            continue;
        };
        let name = obj
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("?");
        lines.push(name.to_string());
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join(", ")
    }
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
        let reasoning = parsed.reasoning_content.unwrap_or_default();
        let name = parsed.name.as_deref().unwrap_or("").trim();

        if role == "system" && name == "crabmate_ui_sep" {
            continue;
        }

        if role == "user" && name == CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME {
            continue;
        }

        if role == "system" && name == "crabmate_timeline" {
            let id = format!("h_{}_{}", base_ms, out.len());
            t = t.saturating_add(1);
            let body = text.trim();
            if let Ok(v) = serde_json::from_str::<Value>(body)
                && let Some(obj) = v.as_object()
            {
                let kind = obj
                    .get("kind")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .trim();
                if kind == "staged_plan_step_started" {
                    let step = obj.get("step_index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    let total =
                        obj.get("total_steps").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    let desc = obj
                        .get("description")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .trim();
                    let exec = obj
                        .get("executor_kind")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let ord = step.max(1);
                    let inner = if let Some(e) = exec {
                        if desc.is_empty() {
                            format!("({e})")
                        } else {
                            format!("{desc}\n({e})")
                        }
                    } else {
                        desc.to_string()
                    };
                    let body = if inner.is_empty() {
                        format!("{ord}.")
                    } else {
                        format!("{ord}. {inner}")
                    };
                    let display = staged_timeline_system_message_body(&body);
                    let state = timeline_state_staged_start(&id, step, total);
                    out.push(StoredMessage {
                        id,
                        role: "system".into(),
                        text: display,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        created_at: t,
                    });
                    continue;
                }
                if kind == "staged_plan_step_finished" {
                    let step = obj.get("step_index").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    let total =
                        obj.get("total_steps").and_then(|x| x.as_u64()).unwrap_or(0) as usize;
                    let status = obj
                        .get("status")
                        .and_then(|x| x.as_str())
                        .unwrap_or("")
                        .to_string();
                    let exec = obj
                        .get("executor_kind")
                        .and_then(|x| x.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    let ord = step.max(1);
                    let inner = if let Some(e) = exec {
                        format!("{status}\n({e})")
                    } else {
                        status.clone()
                    };
                    let body = if inner.trim().is_empty() {
                        format!("{ord}.")
                    } else {
                        format!("{ord}. {inner}")
                    };
                    let display = staged_timeline_system_message_body(&body);
                    let state = timeline_state_staged_end(&id, step, total, &status);
                    out.push(StoredMessage {
                        id,
                        role: "system".into(),
                        text: display,
                        reasoning_text: String::new(),
                        image_urls: vec![],
                        state: Some(state),
                        is_tool: false,
                        created_at: t,
                    });
                    continue;
                }
            }
            let id = format!("h_{}_{}", base_ms, out.len());
            t = t.saturating_add(1);
            let display = staged_timeline_system_message_body(body);
            out.push(StoredMessage {
                id,
                role: "system".into(),
                text: display,
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                created_at: t,
            });
            continue;
        }

        if role == "assistant"
            && parsed.tool_calls.is_some()
            && text.trim().is_empty()
            && reasoning.trim().is_empty()
        {
            let id = format!("h_{}_{}", base_ms, out.len());
            t = t.saturating_add(1);
            let summary = parsed
                .tool_calls
                .as_ref()
                .map(tool_calls_summary)
                .unwrap_or_default();
            let card = if summary.is_empty() {
                "工具调用".to_string()
            } else {
                format!("工具：{summary}")
            };
            let state = timeline_state_tool(&id, true);
            out.push(StoredMessage {
                id,
                role: "system".into(),
                text: card,
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(state),
                is_tool: true,
                created_at: t,
            });
            continue;
        }

        if role == "tool" {
            let id = format!("h_{}_{}", base_ms, out.len());
            t = t.saturating_add(1);
            let state = timeline_state_tool(&id, true);
            out.push(StoredMessage {
                id,
                role: "system".into(),
                text: text.clone(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(state),
                is_tool: true,
                created_at: t,
            });
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
