//! 意图管线用：多轮 user 与「澄清/确认」下的有效 user 句。

use crabmate_types::{Message, message_content_as_str};

use crate::intent_l0;
use crate::intent_router::{
    is_explicit_execute_confirmation, is_waiting_execute_confirmation_prompt,
};

const MSG_TAIL_FOR_TOOL_FAILURE: usize = 24;

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

/// 取当前 user 条**之前**的最近 `max` 条真实 user 正文（**新在前**；跳过编排注入）。
pub fn collect_recent_user_messages(messages: &[Message], max: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for m in messages.iter().rev() {
        if !crabmate_types::is_real_user_task_message(m, false) {
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

fn prior_real_user_task_before_latest(messages: &[Message]) -> Option<String> {
    let mut seen_latest_user = false;
    for m in messages.iter().rev() {
        if !crabmate_types::is_real_user_task_message(m, false) {
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
        return Some(t.to_string());
    }
    None
}

/// 多轮下「请确认 / 请补充」或「失败后续跑」后的**有效**任务句。
pub fn extract_effective_user_task(messages: &[Message], in_clarification_flow: bool) -> String {
    let latest = extract_user_task(messages);
    if !in_clarification_flow {
        if intent_l0::is_resume_after_failure_utterance(latest.trim())
            && intent_l0::messages_have_recent_tool_failure(messages, MSG_TAIL_FOR_TOOL_FAILURE)
            && let Some(prior) = prior_real_user_task_before_latest(messages)
        {
            return prior;
        }
        return latest;
    }
    let latest_norm = latest.trim().to_lowercase();
    if !is_explicit_execute_confirmation(&latest_norm) {
        return latest;
    }

    let mut seen_latest_user = false;
    for m in messages.iter().rev() {
        if !crabmate_types::is_real_user_task_message(m, false) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::Message;

    #[test]
    fn collect_recent_user_messages_skips_orchestration_injection() {
        let messages = vec![
            Message::user_only("先分析目录结构"),
            Message::assistant_only("好的"),
            Message::user_only("编译 hpcg"),
            Message::user_only("【编排纠偏】请实际执行 make"),
        ];
        let recent = collect_recent_user_messages(&messages, 4);
        assert_eq!(recent, vec!["先分析目录结构"]);
    }

    #[test]
    fn extract_effective_user_task_ignores_trailing_orchestration_user() {
        let messages = vec![
            Message::user_only("编译 hpcg"),
            Message::user_only("【编排纠偏】继续构建"),
        ];
        assert_eq!(extract_effective_user_task(&messages, false), "编译 hpcg");
    }

    #[test]
    fn extract_effective_user_task_resume_after_tool_failure() {
        let messages = vec![
            Message::user_only("编写 c++ 程序并用 cmake 编译"),
            Message::assistant_only("开始执行"),
            Message {
                role: "tool".to_string(),
                content: Some(
                    r#"{"crabmate_tool":{"ok":false,"name":"run_command","summary":"失败"}}"#
                        .into(),
                ),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".to_string()),
                tool_call_id: Some("tc1".to_string()),
            },
            Message::user_only("继续"),
        ];
        assert_eq!(
            extract_effective_user_task(&messages, false),
            "编写 c++ 程序并用 cmake 编译"
        );
    }
}
