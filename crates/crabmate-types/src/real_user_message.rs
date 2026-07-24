//! 会话缓冲中「真实用户发言」识别（跳过编排注入、记忆、画像等）。

use crate::message::{Message, message_content_as_str};
use crate::server_injected_user::is_server_injected_user_message;
use crate::{
    is_first_turn_workspace_context_injection, is_message_excluded_from_llm_context_except_memory,
};

/// 是否为计入模型的真实 user 正文（非编排注入、非空白）。
#[inline]
pub fn is_real_user_task_message(m: &Message, skip_first_turn_workspace_bootstrap: bool) -> bool {
    if m.role != "user" {
        return false;
    }
    if is_message_excluded_from_llm_context_except_memory(m) || is_server_injected_user_message(m) {
        return false;
    }
    if skip_first_turn_workspace_bootstrap && is_first_turn_workspace_context_injection(m) {
        return false;
    }
    message_content_as_str(&m.content).is_some_and(|s| !s.trim().is_empty())
}

/// 自后向前：最后一条真实 user 的下标。
#[must_use]
pub fn last_real_user_message_index(
    messages: &[Message],
    skip_first_turn_workspace_bootstrap: bool,
) -> Option<usize> {
    messages.iter().enumerate().rev().find_map(|(i, m)| {
        is_real_user_task_message(m, skip_first_turn_workspace_bootstrap).then_some(i)
    })
}

/// 自最后一条真实 user 起至末尾的消息切片（完成证据、构建空转窗口等共用）。
#[must_use]
pub fn messages_slice_since_last_real_user(
    messages: &[Message],
    skip_first_turn_workspace_bootstrap: bool,
) -> Option<&[Message]> {
    let idx = last_real_user_message_index(messages, skip_first_turn_workspace_bootstrap)?;
    Some(&messages[idx..])
}

/// 自后向前：最后一条真实 user 的正文（与分阶段 `staged_plan_trigger_user_content` 对齐）。
#[must_use]
pub fn last_real_user_task_content(
    messages: &[Message],
    skip_first_turn_workspace_bootstrap: bool,
) -> Option<&str> {
    messages.iter().rev().find_map(|m| {
        if !is_real_user_task_message(m, skip_first_turn_workspace_bootstrap) {
            return None;
        }
        let t = message_content_as_str(&m.content)?.trim();
        (!t.is_empty()).then_some(t)
    })
}

/// 自前向后：第一条真实 user 正文（跳过首轮工作区画像时用于会话级锚点）。
#[must_use]
pub fn first_real_user_task_content(
    messages: &[Message],
    skip_first_turn_workspace_bootstrap: bool,
) -> Option<&str> {
    messages.iter().find_map(|m| {
        if !is_real_user_task_message(m, skip_first_turn_workspace_bootstrap) {
            return None;
        }
        message_content_as_str(&m.content)
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CRABMATE_LONG_TERM_MEMORY_NAME, Message};

    #[test]
    fn skips_orchestration_correction_user_for_last_real_user() {
        let msgs = vec![
            Message::user_only("编译 hpcg"),
            Message::user_only("【编排纠偏】请实际执行 make"),
        ];
        assert_eq!(last_real_user_task_content(&msgs, false), Some("编译 hpcg"));
        assert_eq!(last_real_user_message_index(&msgs, false), Some(0));
    }

    #[test]
    fn slice_since_last_real_user_ignores_trailing_injection() {
        let msgs = vec![
            Message::user_only("分析目录"),
            Message::assistant_only("完成"),
            Message::user_only("编译 hpcg"),
            Message::user_only("【编排纠偏】继续构建"),
            Message::assistant_only("好的"),
        ];
        let window = messages_slice_since_last_real_user(&msgs, false).expect("window");
        assert_eq!(window.len(), 3);
        assert_eq!(
            message_content_as_str(&window[0].content).map(|s| s.trim()),
            Some("编译 hpcg")
        );
    }

    #[test]
    fn multi_turn_last_real_user_is_latest_human_turn() {
        let msgs = vec![
            Message::user_only("分析当前目录"),
            Message::assistant_only("完成"),
            Message::user_only("编译 hpcg"),
        ];
        assert_eq!(last_real_user_task_content(&msgs, false), Some("编译 hpcg"));
    }

    #[test]
    fn skips_named_memory_injection() {
        let msgs = vec![
            Message::user_only("真实问题"),
            Message {
                role: "user".into(),
                content: Some("memo".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some(CRABMATE_LONG_TERM_MEMORY_NAME.into()),
                tool_call_id: None,
            },
        ];
        assert_eq!(last_real_user_task_content(&msgs, false), Some("真实问题"));
    }
}
