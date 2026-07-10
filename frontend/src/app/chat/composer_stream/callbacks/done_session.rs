//! `on_done` 中对会话 **`messages`** 的尾泡收尾（读列表 → [`super::done_bubble::decide_done_bubble_action`] → 写回），
//! 与 **`ChatStreamCallbackCtx::update_bound_session`**（`stream_session_access`）解耦以便单测与降 [`super::builders::chat_stream_on_done_builder`] nloc。

use crate::i18n::Locale;
use crate::storage::StoredMessage;

use super::super::per_stream_accum::PerStreamTurnSummary;
use super::done_bubble::{DoneBubbleAction, DoneBubbleDecisionInputs, decide_done_bubble_action};
use super::helpers::build_empty_reply_with_diagnostic;

fn is_assistant_loading_placeholder(m: &StoredMessage) -> bool {
    m.role == "assistant" && !m.is_tool && m.state.as_ref().is_some_and(|st| st.is_loading())
}

fn push_missing_assistant_diagnostic(
    messages: &mut Vec<StoredMessage>,
    turn: &PerStreamTurnSummary,
    in_answer_body_lane: bool,
    locale: Locale,
) {
    let text = build_empty_reply_with_diagnostic(
        locale,
        in_answer_body_lane,
        turn.answer_delta_chars,
        turn.stream_end_reason.as_deref(),
    );
    messages.push(StoredMessage {
        id: format!("asst_diag_{}", messages.len()),
        role: "assistant".to_string(),
        text,
        reasoning_text: String::new(),
        image_urls: Vec::new(),
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: 0,
    });
}

/// 流式整轮结束后仍残留的助手 `Loading` 占位（轮换/id 漂移等）须在此清除，避免 UI 与 lifecycle 长期不一致。
pub(super) fn clear_residual_assistant_loading_placeholders(messages: &mut Vec<StoredMessage>) {
    messages.retain(|m| !is_assistant_loading_placeholder(m));
}

/// 在会话消息列表上对 `assistant_message_id` 指向的 **loading** 尾泡应用 `on_done` 决策。
pub(super) fn apply_stream_done_to_loading_assistant(
    messages: &mut Vec<StoredMessage>,
    assistant_message_id: &str,
    turn: &PerStreamTurnSummary,
    in_answer_body_lane: bool,
    locale: Locale,
) {
    let has_hierarchical_or_tool = messages.iter().any(|x| {
        x.is_tool
            || x.state
                .as_ref()
                .is_some_and(|st| st.looks_like_hierarchical_subgoal())
    });
    let Some(idx) = messages.iter().position(|m| m.id == assistant_message_id) else {
        clear_residual_assistant_loading_placeholders(messages);
        if turn.answer_delta_chars == 0 && !turn.saw_final_response_timeline {
            push_missing_assistant_diagnostic(messages, turn, in_answer_body_lane, locale);
        }
        return;
    };
    if !is_assistant_loading_placeholder(&messages[idx]) {
        clear_residual_assistant_loading_placeholders(messages);
        return;
    }
    messages[idx].state = None;
    let body_chars =
        messages[idx].text.chars().count() + messages[idx].reasoning_text.chars().count();
    let diag_chars = body_chars.max(turn.answer_delta_chars);
    let body_and_reasoning_empty =
        messages[idx].text.trim().is_empty() && messages[idx].reasoning_text.trim().is_empty();
    let end_reason = turn.stream_end_reason.as_deref();
    match decide_done_bubble_action(DoneBubbleDecisionInputs {
        body_and_reasoning_empty,
        end_reason_raw: end_reason,
        in_answer_body_lane,
        diag_chars,
        has_hierarchical_or_tool,
        saw_final_response_timeline: turn.saw_final_response_timeline,
    }) {
        DoneBubbleAction::Keep => {}
        DoneBubbleAction::RemoveBubble => {
            messages.remove(idx);
        }
        DoneBubbleAction::FillMissingFinalHint => {
            messages[idx].text = format!(
                "{}\n\n{}",
                crate::i18n::stream_completed_missing_final_summary_hint(locale),
                crate::i18n::stream_empty_reply_diag_line(
                    locale,
                    end_reason,
                    in_answer_body_lane,
                    diag_chars,
                )
            );
        }
        DoneBubbleAction::FillDiagnostic => {
            messages[idx].text = build_empty_reply_with_diagnostic(
                locale,
                in_answer_body_lane,
                diag_chars,
                end_reason,
            );
        }
    }
    clear_residual_assistant_loading_placeholders(messages);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn loading_asst(id: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: String::new(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn clears_orphan_loading_when_primary_id_missing() {
        let mut msgs = vec![loading_asst("orphan")];
        apply_stream_done_to_loading_assistant(
            &mut msgs,
            "missing",
            &PerStreamTurnSummary {
                answer_delta_chars: 0,
                stream_end_reason: None,
                saw_final_response_timeline: false,
                current_subgoal_marker: None,
            },
            false,
            crate::i18n::Locale::ZhHans,
        );
        assert_eq!(msgs.len(), 1);
        assert!(msgs[0].text.contains("未进入正文阶段"));
    }

    #[test]
    fn clears_extra_loading_after_primary_done() {
        let mut msgs = vec![loading_asst("orphan"), loading_asst("primary")];
        apply_stream_done_to_loading_assistant(
            &mut msgs,
            "primary",
            &PerStreamTurnSummary {
                answer_delta_chars: 0,
                stream_end_reason: Some("completed".into()),
                saw_final_response_timeline: false,
                current_subgoal_marker: None,
            },
            true,
            crate::i18n::Locale::ZhHans,
        );
        assert!(msgs.is_empty());
    }
}
