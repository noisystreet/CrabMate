//! 单条助手消息「可见输出类别」的单一事实来源。

use super::{Message, assistant_has_non_empty_tool_calls, message_content_as_str};

/// 单条助手消息「可见输出类别」的单一事实来源。
///
/// 供结束原因判定（`cm_llm::api::stream_end_reason_from_finish_and_message`）与任务收尾
/// （`chat_job_queue::stream_finish::current_turn_has_visible_assistant_output`）共用，避免两家
/// 对「思维链算不算输出」的口径漂移：**仅有思维链不算可见终答**，否则会出现
/// `stream_ended=completed` 却 `answer_phase=false`、无 `final_response` 的自相矛盾收尾。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantOutputKind {
    /// 非 `assistant` 角色，或无正文、无思维链、无工具调用。
    Empty,
    /// 仅有思维链（`reasoning_content`）：无可见正文、无工具调用。
    ReasoningOnly,
    /// 有工具调用（可能同时带正文 / 思维链）。
    ToolCalls,
    /// 有可见正文（无工具调用）。
    Answer,
}

impl AssistantOutputKind {
    /// 是否算作「已产出可见输出」，用于结束原因与任务收尾的口径统一。
    ///
    /// [`Self::Answer`] / [`Self::ToolCalls`] 为真；**`ReasoningOnly` 为假**——思维链是过程信息，
    /// 未进入正文阶段时不能据此判定 `completed`。
    #[inline]
    #[must_use]
    pub fn counts_as_visible_output(self) -> bool {
        matches!(
            self,
            AssistantOutputKind::Answer | AssistantOutputKind::ToolCalls
        )
    }
}

/// 判定单条消息的可见输出类别（见 [`AssistantOutputKind`]）。
#[inline]
#[must_use]
pub fn assistant_output_kind(m: &Message) -> AssistantOutputKind {
    if m.role != "assistant" {
        return AssistantOutputKind::Empty;
    }
    if assistant_has_non_empty_tool_calls(m) {
        return AssistantOutputKind::ToolCalls;
    }
    let has_answer = message_content_as_str(&m.content)
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_answer {
        return AssistantOutputKind::Answer;
    }
    let has_reasoning = m
        .reasoning_content
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_reasoning {
        return AssistantOutputKind::ReasoningOnly;
    }
    AssistantOutputKind::Empty
}

#[cfg(test)]
mod assistant_output_kind_tests {
    use super::*;
    use crate::types::{FunctionCall, MessageContent, ToolCall};

    fn with_reasoning(mut m: Message, reasoning: &str) -> Message {
        m.reasoning_content = Some(reasoning.to_string());
        m
    }

    #[test]
    fn classifies_answer_tool_calls_reasoning_only_and_empty() {
        assert_eq!(
            assistant_output_kind(&Message::assistant_only("终答正文")),
            AssistantOutputKind::Answer
        );

        let mut with_tc = Message::assistant_only("");
        with_tc.content = None;
        with_tc.tool_calls = Some(vec![ToolCall {
            id: "call_1".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        }]);
        assert_eq!(
            assistant_output_kind(&with_tc),
            AssistantOutputKind::ToolCalls
        );

        // 仅有思维链：无正文、无工具调用。
        let mut reasoning_only = Message::assistant_only("");
        reasoning_only.content = None;
        let reasoning_only = with_reasoning(reasoning_only, "先分析一下…");
        assert_eq!(
            assistant_output_kind(&reasoning_only),
            AssistantOutputKind::ReasoningOnly
        );

        // 空白正文等同无正文。
        let mut blank = Message::assistant_only("   ");
        blank.content = Some(MessageContent::Text("   ".to_string()));
        assert_eq!(assistant_output_kind(&blank), AssistantOutputKind::Empty);

        // 非 assistant 角色一律 Empty。
        assert_eq!(
            assistant_output_kind(&Message::user_only("hi")),
            AssistantOutputKind::Empty
        );
    }

    #[test]
    fn reasoning_only_does_not_count_as_visible_output() {
        let mut reasoning_only = Message::assistant_only("");
        reasoning_only.content = None;
        let reasoning_only = with_reasoning(reasoning_only, "先分析一下…");
        assert!(!assistant_output_kind(&reasoning_only).counts_as_visible_output());

        assert!(assistant_output_kind(&Message::assistant_only("终答")).counts_as_visible_output());
    }

    #[test]
    fn tool_calls_take_precedence_over_blank_content() {
        let mut m = Message::assistant_only("");
        m.content = Some(MessageContent::Text("  ".to_string()));
        m.tool_calls = Some(vec![ToolCall {
            id: "call_2".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "list_dir".to_string(),
                arguments: "{}".to_string(),
            },
        }]);
        assert_eq!(assistant_output_kind(&m), AssistantOutputKind::ToolCalls);
    }
}
