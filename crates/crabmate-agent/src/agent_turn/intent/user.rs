//! 意图管线用：多轮 user 与「澄清/确认」下的有效 user 句。

use crabmate_types::{Message, message_content_as_str};

use crate::intent_router::{
    is_explicit_execute_confirmation, is_waiting_execute_confirmation_prompt,
};

pub fn recently_waiting_execute_confirmation(messages: &[Message]) -> bool {
    messages.iter().rev().take(4).any(|m| {
        if m.role != "assistant" {
            return false;
        }
        let Some(content) = message_content_as_str(&m.content) else {
            return false;
        };
        is_waiting_execute_confirmation_prompt(content)
    })
}

/// 取当前 user 条**之前**的最近 `max` 条 user 正文（**新在前**）。
pub fn collect_recent_user_messages(messages: &[Message], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in messages.iter().rev() {
        if m.role != "user" {
            continue;
        }
        if out.len() > max {
            break;
        }
        if let Some(t) = message_content_as_str(&m.content) {
            let s = t.trim();
            if !s.is_empty() {
                out.push(s.to_string());
            }
        }
    }
    if out.is_empty() {
        return Vec::new();
    }
    out.remove(0);
    out
}

fn extract_user_task(messages: &[Message]) -> String {
    crabmate_types::last_real_user_task_content(messages, false)
        .unwrap_or_default()
        .to_string()
}

/// 多轮下「请确认 / 请补充」后用户续接的**有效**任务句。
pub fn extract_effective_user_task(messages: &[Message], in_clarification_flow: bool) -> String {
    let latest = extract_user_task(messages);
    if !in_clarification_flow {
        return latest;
    }
    let latest_norm = latest.trim().to_lowercase();
    if !is_explicit_execute_confirmation(&latest_norm) {
        return latest;
    }

    let mut seen_latest_user = false;
    for m in messages.iter().rev() {
        if m.role != "user" {
            continue;
        }
        let Some(content) = message_content_as_str(&m.content) else {
            continue;
        };
        let t = content.trim();
        if t.is_empty() {
            continue;
        }
        if !seen_latest_user {
            seen_latest_user = true;
            continue;
        }
        let norm = t.to_lowercase();
        if !is_explicit_execute_confirmation(&norm) {
            return t.to_string();
        }
    }
    latest
}
