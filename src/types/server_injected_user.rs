//! 服务端注入的 **`role: user`** 消息：注册表、识别与落盘剥离（非用户真实发言）。

use crate::types::message::{
    CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_LONG_TERM_MEMORY_NAME,
    CRABMATE_PLAN_REWRITE_NAME, CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME,
    CRABMATE_STAGED_NL_FOLLOWUP_NAME, CRABMATE_STAGED_PATCH_FEEDBACK_NAME,
    CRABMATE_STAGED_PLAN_COACH_NAME, CRABMATE_STAGED_STEP_INJECTION_NAME,
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
            | Some(CRABMATE_STAGED_STEP_INJECTION_NAME)
            | Some(CRABMATE_STAGED_PLAN_COACH_NAME)
            | Some(CRABMATE_STAGED_NL_FOLLOWUP_NAME)
            | Some(CRABMATE_STAGED_PATCH_FEEDBACK_NAME)
            | Some(CRABMATE_PLAN_REWRITE_NAME)
    )
}

/// 分阶段 / 补丁 / ensemble 等临时 coach user（取消或失败时弹出）。
#[inline]
pub fn is_ephemeral_staged_coach_user_message(m: &Message) -> bool {
    if m.role != "user" {
        return false;
    }
    if matches!(
        m.name.as_deref(),
        Some(CRABMATE_STAGED_PLAN_COACH_NAME)
            | Some(CRABMATE_STAGED_PATCH_FEEDBACK_NAME)
            | Some(CRABMATE_STAGED_NL_FOLLOWUP_NAME)
            | Some(CRABMATE_PLANNER_TOOL_CALL_REJECT_NAME)
    ) {
        return true;
    }
    message_content_as_str(&m.content).is_some_and(|c| {
        crabmate_display_rules::is_ensemble_injected_user_content(c)
            || c.contains(crabmate_display_rules::STAGED_PLAN_OPTIMIZER_COACH_MARK)
            || crabmate_display_rules::is_staged_nl_followup_bridge_user_content(c)
    })
}

/// 根据分阶段注入正文选择 `user.name`（构造 [`Message::user_staged_orchestration_injection`] 用）。
#[must_use]
pub fn staged_injection_user_name_for_content(content: &str) -> &'static str {
    let t = content.trim_start();
    if crabmate_display_rules::is_staged_step_injection_user_pattern(content) {
        return CRABMATE_STAGED_STEP_INJECTION_NAME;
    }
    if crabmate_display_rules::is_staged_nl_followup_bridge_user_content(content) {
        return CRABMATE_STAGED_NL_FOLLOWUP_NAME;
    }
    if t.starts_with("### 分阶段规划 · 步级反馈") {
        return CRABMATE_STAGED_PATCH_FEEDBACK_NAME;
    }
    if crabmate_display_rules::is_ensemble_injected_user_content(content)
        || t.contains(crabmate_display_rules::STAGED_PLAN_OPTIMIZER_COACH_MARK)
        || crabmate_display_rules::is_staged_plan_coach_injected_user_content(content)
    {
        return CRABMATE_STAGED_PLAN_COACH_NAME;
    }
    CRABMATE_STAGED_PLAN_COACH_NAME
}
