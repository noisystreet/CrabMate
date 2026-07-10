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

/// 分步窗口 `[step_user_index, end)` 内全部 `role: tool` 消息（时间序）。
#[must_use]
pub fn tool_messages_in_staged_step_window(
    messages: &[Message],
    step_user_index: usize,
) -> Vec<&Message> {
    if step_user_index >= messages.len() {
        return Vec::new();
    }
    let end = staged_step_window_end_exclusive(messages, step_user_index);
    let mut tools = Vec::new();
    let mut i = step_user_index.saturating_add(1);
    while i < end {
        if messages[i].role == "tool" {
            tools.push(&messages[i]);
        }
        i += 1;
    }
    tools
}

/// **步 episode** 终点（不含）：自 `episode_start_index` 起至缓冲末尾，或下一条**真实 user**（不含编排/分步注入）。
///
/// 用于补丁重试 / 新分步注入后仍认可**同一步下标**上先前已产生的 `role: tool`（见 `tool_messages_in_staged_step_episode`）。
#[must_use]
pub fn staged_step_episode_end_exclusive(
    messages: &[Message],
    episode_start_index: usize,
) -> usize {
    if episode_start_index >= messages.len() {
        return messages.len();
    }
    let mut i = episode_start_index.saturating_add(1);
    while i < messages.len() {
        if is_real_user_task_message(&messages[i], false) {
            break;
        }
        i += 1;
    }
    i
}

/// 步 episode `[episode_start_index, end)` 内全部 `role: tool`（跨多次分步注入与补丁重试）。
#[must_use]
pub fn tool_messages_in_staged_step_episode(
    messages: &[Message],
    episode_start_index: usize,
) -> Vec<&Message> {
    if episode_start_index >= messages.len() {
        return Vec::new();
    }
    let end = staged_step_episode_end_exclusive(messages, episode_start_index);
    let mut tools = Vec::new();
    let mut i = episode_start_index.saturating_add(1);
    while i < end {
        if messages[i].role == "tool" {
            tools.push(&messages[i]);
        }
        i += 1;
    }
    tools
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

    #[test]
    fn tool_messages_in_window_excludes_next_step() {
        let msgs = vec![
            step_user("a"),
            Message {
                role: "tool".into(),
                content: Some("t1".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".into()),
                tool_call_id: Some("tc1".into()),
            },
            step_user("b"),
            Message {
                role: "tool".into(),
                content: Some("t2".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".into()),
                tool_call_id: Some("tc2".into()),
            },
        ];
        assert_eq!(tool_messages_in_staged_step_window(&msgs, 0).len(), 1);
        assert_eq!(tool_messages_in_staged_step_window(&msgs, 2).len(), 1);
    }

    #[test]
    fn episode_includes_tools_across_retry_injection() {
        let msgs = vec![
            Message::user_only("task"),
            step_user("restructure"),
            Message {
                role: "tool".into(),
                content: Some("退出码：0\n标准输出：\n100% tests passed\n".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".into()),
                tool_call_id: Some("tc1".into()),
            },
            Message::user_staged_orchestration_injection("### 分阶段规划 · 步级反馈\n补丁"),
            step_user("run-and-verify-v2"),
            Message::assistant_only("summary only"),
        ];
        assert_eq!(tool_messages_in_staged_step_window(&msgs, 1).len(), 1);
        assert!(tool_messages_in_staged_step_window(&msgs, 4).is_empty());
        assert_eq!(tool_messages_in_staged_step_episode(&msgs, 1).len(), 1);
    }

    #[test]
    fn episode_stops_at_real_user_not_patch_injection() {
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
            Message::user_only("new human turn"),
            Message {
                role: "tool".into(),
                content: Some("t2".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("tc2".into()),
            },
        ];
        assert_eq!(tool_messages_in_staged_step_episode(&msgs, 0).len(), 1);
    }
}
