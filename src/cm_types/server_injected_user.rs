//! 服务端注入的 **`role: user`** 消息：注册表、识别与落盘剥离（非用户真实发言）。

use crate::cm_types::message::{
    CRABMATE_AUTO_RESUME_NAME, CRABMATE_CONTEXT_SUMMARY_NAME,
    CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_LONG_TERM_MEMORY_NAME,
    CRABMATE_PAUSED_TASK_NAME, CRABMATE_PLAN_REWRITE_NAME, CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME,
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
        .is_some_and(crate::cm_display_rules::user_message_should_hide_for_chat_display)
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
            | Some(CRABMATE_CONTEXT_SUMMARY_NAME)
            | Some(CRABMATE_PAUSED_TASK_NAME)
            | Some(CRABMATE_AUTO_RESUME_NAME)
    )
}

/// **由运行时每次 prepare 重新注入**的那一类（落盘里的旧副本必须剥离，否则会与
/// 最新状态各说一套）。这是新增注入类型时**唯一**需要登记的地方。
///
/// 与 [`is_server_injected_user_message`] 的区别：后者还包含**要发给模型**的注入
/// （首轮工作区画像、上下文摘要、plan rewrite 等），把它们一并剔除会改变模型输入。
pub fn is_runtime_refreshed_injection(m: &Message) -> bool {
    crate::cm_types::is_long_term_memory_injection(m)
        || crate::cm_types::is_workspace_changelist_injection(m)
        || crate::cm_types::is_paused_task_injection(m)
        // 自动续跑指令：每个续跑回合重新生成，落盘副本无意义（且会把「机器代发」写进历史）。
        || crate::cm_types::is_auto_resume_injection(m)
}

/// 落盘 / 快照前剥离「运行时刷新类」注入（LTM / 变更集 / 暂停任务摘要）。
///
/// 挂这里而不是各自实现：新增一类刷新型注入时只改 [`is_runtime_refreshed_injection`]，
/// 所有剥离点自动跟上。
pub fn strip_runtime_refreshed_injections(messages: &mut Vec<Message>) {
    messages.retain(|m| !is_runtime_refreshed_injection(m));
}

/// 落盘前剥离：编排类注入 user（保留首轮工作区画像、上下文摘要等仍须持久化的注入）。
pub fn strip_orchestration_injected_users_for_conversation_store(messages: &mut Vec<Message>) {
    messages.retain(|m| !should_strip_user_before_conversation_store(m));
}

#[inline]
fn should_strip_user_before_conversation_store(m: &Message) -> bool {
    m.role == "user"
        && is_server_injected_user_message(m)
        && !crate::cm_types::is_first_turn_workspace_context_injection(m)
        && !crate::cm_types::is_context_summary_injection(m)
}
