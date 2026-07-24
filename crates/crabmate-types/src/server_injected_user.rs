//! 服务端注入的 **`role: user`** 消息：注册表、识别与落盘剥离（非用户真实发言）。

use crate::message::{
    CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_LONG_TERM_MEMORY_NAME,
    CRABMATE_PLAN_REWRITE_NAME, CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME,
    CRABMATE_WORKSPACE_CHANGELIST_NAME, Message, message_content_as_str,
};

/// 是否为 **`role: user`** 且非用户真实发言（编排注入、画像、记忆等）。
#[inline]
pub fn is_server_injected_user_message(m: &Message) -> bool {
    if m.role != "user" {
        return false;
    }
    if is_server_injected_user_by_name(m) {
        return true;
    }
    message_content_as_str(&m.content)
        .is_some_and(crabmate_display_rules::is_server_injected_user_content_for_storage)
}

#[inline]
fn is_server_injected_user_by_name(m: &Message) -> bool {
    matches!(
        m.name.as_deref(),
        Some(CRABMATE_LONG_TERM_MEMORY_NAME)
            | Some(CRABMATE_WORKSPACE_CHANGELIST_NAME)
            | Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME)
            | Some(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME)
            | Some(CRABMATE_PLAN_REWRITE_NAME)
    )
}

/// 落盘前剥离：编排类注入 user（保留首轮工作区画像等仍须持久化的注入）。
pub fn strip_orchestration_injected_users_for_conversation_store(messages: &mut Vec<Message>) {
    messages.retain(|m| !should_strip_user_before_conversation_store(m));
}

#[inline]
fn should_strip_user_before_conversation_store(m: &Message) -> bool {
    m.role == "user"
        && is_server_injected_user_message(m)
        && !crate::is_first_turn_workspace_context_injection(m)
}
