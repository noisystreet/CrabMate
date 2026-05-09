//! 会话内 [`super::Message`] 的**血缘/来源**分类（调试、导出过滤、回放策略）；**不**写入 OpenAI 兼容 JSON。
//!
//! 与现有约定对齐：`role` + 可选 `name`（如 [`super::CRABMATE_LONG_TERM_MEMORY_NAME`]）仍是权威存储；
//! [`message_lineage`] 为派生视图，避免各处散落字符串比较。
#![allow(dead_code)]

/// 非 API 序列化字段：由 [`super::Message`] 的 `role` / `name` / `tool_call_id` 等推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextInjectionKind {
    LongTermMemory,
    WorkspaceChangelist,
    FirstTurnWorkspaceContext,
    IntentGateEphemeral,
    /// 其它带 `user.name` / `system.name` 的注入（未来扩展）。
    Other,
}

/// 单条消息在 CrabMate 会话模型中的来源类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageLineage {
    /// 用户自然输入（不含注入 `name`）。
    UserNatural,
    AssistantModel,
    ToolResult,
    /// 普通 system（含系统提示词主体）。
    SystemPlain,
    UiSeparator,
    UiTimeline,
    ContextInjection(ContextInjectionKind),
    Unknown,
}

/// 由单条消息推导来源类别（与落盘 JSON 字段兼容，无额外存储）。
#[must_use]
pub fn message_lineage(m: &super::Message) -> MessageLineage {
    use super::{
        is_chat_timeline_marker, is_chat_ui_separator, is_first_turn_workspace_context_injection,
        is_intent_gate_ephemeral_system, is_long_term_memory_injection,
        is_workspace_changelist_injection,
    };

    if is_chat_ui_separator(m) {
        return MessageLineage::UiSeparator;
    }
    if is_chat_timeline_marker(m) {
        return MessageLineage::UiTimeline;
    }
    if m.role == "tool" {
        return MessageLineage::ToolResult;
    }
    if is_long_term_memory_injection(m) {
        return MessageLineage::ContextInjection(ContextInjectionKind::LongTermMemory);
    }
    if is_workspace_changelist_injection(m) {
        return MessageLineage::ContextInjection(ContextInjectionKind::WorkspaceChangelist);
    }
    if is_first_turn_workspace_context_injection(m) {
        return MessageLineage::ContextInjection(ContextInjectionKind::FirstTurnWorkspaceContext);
    }
    if is_intent_gate_ephemeral_system(m) {
        return MessageLineage::ContextInjection(ContextInjectionKind::IntentGateEphemeral);
    }
    match m.role.as_str() {
        "user" => {
            if m.name.is_some() {
                MessageLineage::ContextInjection(ContextInjectionKind::Other)
            } else {
                MessageLineage::UserNatural
            }
        }
        "assistant" => MessageLineage::AssistantModel,
        "system" => MessageLineage::SystemPlain,
        _ => MessageLineage::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        CRABMATE_LONG_TERM_MEMORY_NAME, CRABMATE_WORKSPACE_CHANGELIST_NAME, Message, MessageContent,
    };

    #[test]
    fn lineage_user_vs_memory_injection() {
        let u = Message::user_only("hi");
        assert_eq!(message_lineage(&u), MessageLineage::UserNatural);

        let inj = Message {
            role: "user".into(),
            content: Some(MessageContent::Text("x".into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_LONG_TERM_MEMORY_NAME.to_string()),
            tool_call_id: None,
        };
        assert_eq!(
            message_lineage(&inj),
            MessageLineage::ContextInjection(ContextInjectionKind::LongTermMemory)
        );
    }

    #[test]
    fn lineage_changelist_and_tool() {
        let cl = Message {
            role: "user".into(),
            content: Some(MessageContent::Text("cl".into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(CRABMATE_WORKSPACE_CHANGELIST_NAME.to_string()),
            tool_call_id: None,
        };
        assert_eq!(
            message_lineage(&cl),
            MessageLineage::ContextInjection(ContextInjectionKind::WorkspaceChangelist)
        );

        let tool = Message {
            role: "tool".into(),
            content: Some(MessageContent::Text("ok".into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("call_1".into()),
        };
        assert_eq!(message_lineage(&tool), MessageLineage::ToolResult);
    }
}
