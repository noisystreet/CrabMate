//! 助手消息合并与「本轮用户后」UI 分隔线插入。
//!
//! 分阶段规划教练 / ensemble 注入的临时 **user** 弹出逻辑见 [`pop_last_staged_planner_coach_user_if_present`]（底层仅操作 `Vec`；
//! 回合侧请用 [`super::params::RunLoopTurnState::pop_last_staged_planner_coach_user_if_present`] 以同步 **`messages_revision`**）。

use crate::agent::plan_ensemble;
use crate::agent::plan_optimizer::STAGED_PLAN_OPTIMIZER_COACH_MARK;
use crate::types::{Message, is_chat_ui_separator, message_content_as_str};

pub(crate) fn push_assistant_merging_trailing_empty_placeholder(
    messages: &mut Vec<Message>,
    msg: Message,
) {
    if msg.role != "assistant" {
        messages.push(msg);
        return;
    }
    if let Some(last) = messages.last_mut()
        && last.role == "assistant"
        && last.tool_calls.is_none()
        && message_content_as_str(&last.content)
            .map(|s| s.trim())
            .unwrap_or("")
            .is_empty()
    {
        *last = msg;
        return;
    }
    messages.push(msg);
}

pub(crate) fn insert_separator_after_last_user_for_turn(messages: &mut Vec<Message>) {
    let Some(user_idx) = messages.iter().rposition(|m| m.role == "user") else {
        return;
    };
    if messages.get(user_idx + 1).is_some_and(is_chat_ui_separator) {
        return;
    }
    let sep = Message::chat_ui_separator(true);
    match messages.get(user_idx + 1) {
        Some(m) if m.role == "assistant" => {
            messages.insert(user_idx + 1, sep);
        }
        _ => {
            messages.push(sep);
        }
    }
}

/// 若最后一条为带「规划教练」标记的临时 user，则弹出（取消或解析失败时避免孤立上下文）。
pub(crate) fn pop_last_staged_planner_coach_user_if_present(messages: &mut Vec<Message>) {
    if let Some(last) = messages.last()
        && last.role == "user"
        && crate::types::message_content_as_str(&last.content).is_some_and(|c| {
            c.contains(STAGED_PLAN_OPTIMIZER_COACH_MARK)
                || plan_ensemble::is_ensemble_injected_user_content(c)
        })
    {
        messages.pop();
    }
}

/// 规划轮 tool_calls 拒绝重写约束 user：重写 LLM 调用完成后弹出，避免落盘/Web 水合污染真实用户气泡。
pub(crate) fn pop_last_planner_tool_call_reject_user_if_present(messages: &mut Vec<Message>) {
    if let Some(last) = messages.last()
        && crate::types::is_planner_tool_call_reject_injection(last)
    {
        messages.pop();
    }
}
