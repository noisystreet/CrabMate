//! 分阶段单步消息窗口：自分步 `user` 起至下一**步界**（真实 user 或下一条分步注入）止。
//!
//! 步内 **`user_staged_orchestration_injection`**（外循环纠偏、coach 等）**不**截断窗口。

use crate::message::{CRABMATE_STAGED_STEP_INJECTION_NAME, Message, message_content_as_str};
use crate::real_user_message::is_real_user_task_message;

/// 是否为分阶段「分步注入」`user`（新步起点）。
#[inline]
pub fn is_staged_step_injection_user_message(m: &Message) -> bool {
    if m.role != "user" {
        return false;
    }
    if m.name.as_deref() == Some(CRABMATE_STAGED_STEP_INJECTION_NAME) {
        return true;
    }
    message_content_as_str(&m.content)
        .is_some_and(crabmate_display_rules::is_staged_step_injection_user_pattern)
}

/// 自 `step_user_index` 之后的消息是否构成步界终止（真实 user 或下一条分步注入）。
#[inline]
pub fn is_staged_step_window_boundary_user(
    m: &Message,
    message_index: usize,
    step_user_index: usize,
) -> bool {
    if m.role != "user" || message_index <= step_user_index {
        return false;
    }
    if is_real_user_task_message(m, false) {
        return true;
    }
    is_staged_step_injection_user_message(m)
}

/// 分步窗口 `[step_user_index, end)` 的 **`end`**（不含）；`step_user_index` 越界时返回 `messages.len()`。
#[must_use]
pub fn staged_step_window_end_exclusive(messages: &[Message], step_user_index: usize) -> usize {
    if step_user_index >= messages.len() {
        return messages.len();
    }
    let mut i = step_user_index.saturating_add(1);
    while i < messages.len() {
        if is_staged_step_window_boundary_user(&messages[i], i, step_user_index) {
            break;
        }
        i += 1;
    }
    i
}

/// 缓冲内最后一条分步注入 `user` 的下标（步后重规划等共用）。
#[must_use]
pub fn last_staged_step_injection_index(messages: &[Message]) -> Option<usize> {
    messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, m)| is_staged_step_injection_user_message(m).then_some(i))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Message;

    fn step_user(body: &str) -> Message {
        Message::user_staged_step_injection(format!("### 分步 1/1\n- id: s1\n- 描述: {body}"))
    }

    #[test]
    fn orchestration_user_does_not_close_step_window() {
        let msgs = vec![
            Message::user_only("goal"),
            step_user("build"),
            Message::user_staged_orchestration_injection("【编排纠偏】请继续构建"),
            Message {
                role: "tool".into(),
                content: Some("ok".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".into()),
                tool_call_id: Some("tc1".into()),
            },
        ];
        assert_eq!(staged_step_window_end_exclusive(&msgs, 1), msgs.len());
    }

    #[test]
    fn next_step_injection_closes_window() {
        let msgs = vec![
            step_user("a"),
            Message {
                role: "tool".into(),
                content: Some("t1".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".into()),
            },
            step_user("b"),
        ];
        assert_eq!(staged_step_window_end_exclusive(&msgs, 0), 2);
    }

    #[test]
    fn real_user_closes_window() {
        let msgs = vec![
            step_user("a"),
            Message {
                role: "tool".into(),
                content: Some("t1".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc1".into()),
            },
            Message::user_only("follow up"),
        ];
        assert_eq!(staged_step_window_end_exclusive(&msgs, 0), 2);
    }
}
