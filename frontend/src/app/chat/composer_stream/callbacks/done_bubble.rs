//! `on_done` 收尾时对「尾泡 loading」的处置：**纯函数决策** + 单元测试。
//!
//! 将隐式状态机从闭包中剥离：`stream_end_reason`、输出车道、`diag_chars`、时间轴标记、
//! 「正文/reasoning 是否皆空」等组合为 [`DoneBubbleDecisionInputs`]，
//! 由 [`decide_done_bubble_action`] 返回 [`DoneBubbleAction`]；
//! [`super::builders::chat_stream_on_done_builder`] 仅负责读信号、**`ChatStreamCallbackCtx::update_bound_session`**（`stream_session_access` 上实现）、`scratch` 等、执行动作。

use crabmate_sse_protocol::StreamEndReason;

use super::helpers::should_show_missing_final_summary_hint;

fn done_bubble_parsed_end_reason(raw: Option<&str>) -> Option<StreamEndReason> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

fn done_remove_completed_with_visible_delta(
    parsed: Option<StreamEndReason>,
    inp: &DoneBubbleDecisionInputs<'_>,
) -> bool {
    parsed == Some(StreamEndReason::Completed) && inp.in_answer_body_lane && inp.diag_chars > 0
}

fn done_remove_completed_plain_empty(
    parsed: Option<StreamEndReason>,
    inp: &DoneBubbleDecisionInputs<'_>,
) -> bool {
    parsed == Some(StreamEndReason::Completed)
        && inp.diag_chars == 0
        && !inp.has_hierarchical_or_tool
}

fn done_remove_empty_main_after_tool_like_turn(
    parsed: Option<StreamEndReason>,
    inp: &DoneBubbleDecisionInputs<'_>,
) -> bool {
    let Some(r) = parsed else {
        return false;
    };
    matches!(
        r,
        StreamEndReason::Completed
            | StreamEndReason::Fallback
            | StreamEndReason::Cancelled
            | StreamEndReason::NoOutput
    ) && inp.has_hierarchical_or_tool
        && (!inp.in_answer_body_lane || inp.diag_chars == 0)
}

fn done_remove_redundant_empty_after_fallback_timeline(
    parsed: Option<StreamEndReason>,
    inp: &DoneBubbleDecisionInputs<'_>,
) -> bool {
    (parsed == Some(StreamEndReason::Fallback) || inp.saw_final_response_timeline)
        && inp.in_answer_body_lane
        && inp.diag_chars == 0
}

/// 决策输入：须与 `chat_stream_on_done_builder` 里收集的信号一致，便于对照协议语义。
#[derive(Clone, Copy)]
pub(super) struct DoneBubbleDecisionInputs<'a> {
    /// `text.trim()` 与 `reasoning_text.trim()` 均为空时为 `true`。
    pub body_and_reasoning_empty: bool,
    pub end_reason_raw: Option<&'a str>,
    pub in_answer_body_lane: bool,
    pub diag_chars: usize,
    pub has_hierarchical_or_tool: bool,
    pub saw_final_response_timeline: bool,
}

/// 对尾泡应采取的动作（不含 i18n 拼装与会话写入，由 `on_done` 执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DoneBubbleAction {
    /// 已有可见正文或 reasoning：仅清除 `loading`（调用方已写 `state = None`）。
    Keep,
    RemoveBubble,
    FillMissingFinalHint,
    FillDiagnostic,
}

pub(super) fn decide_done_bubble_action(inp: DoneBubbleDecisionInputs<'_>) -> DoneBubbleAction {
    if !inp.body_and_reasoning_empty {
        return DoneBubbleAction::Keep;
    }
    let parsed = done_bubble_parsed_end_reason(inp.end_reason_raw);
    if done_remove_completed_with_visible_delta(parsed, &inp) {
        return DoneBubbleAction::RemoveBubble;
    }
    if done_remove_completed_plain_empty(parsed, &inp) {
        return DoneBubbleAction::RemoveBubble;
    }
    if done_remove_empty_main_after_tool_like_turn(parsed, &inp) {
        return DoneBubbleAction::RemoveBubble;
    }
    if done_remove_redundant_empty_after_fallback_timeline(parsed, &inp) {
        return DoneBubbleAction::RemoveBubble;
    }
    if should_show_missing_final_summary_hint(
        inp.end_reason_raw,
        inp.in_answer_body_lane,
        inp.has_hierarchical_or_tool,
        inp.saw_final_response_timeline,
    ) {
        DoneBubbleAction::FillMissingFinalHint
    } else {
        DoneBubbleAction::FillDiagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_when_body_present() {
        let a = decide_done_bubble_action(DoneBubbleDecisionInputs {
            body_and_reasoning_empty: false,
            end_reason_raw: Some("completed"),
            in_answer_body_lane: true,
            diag_chars: 0,
            has_hierarchical_or_tool: true,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, DoneBubbleAction::Keep);
    }

    #[test]
    fn completed_with_answer_lane_and_diag_removes() {
        let a = decide_done_bubble_action(DoneBubbleDecisionInputs {
            body_and_reasoning_empty: true,
            end_reason_raw: Some("completed"),
            in_answer_body_lane: true,
            diag_chars: 3,
            has_hierarchical_or_tool: false,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, DoneBubbleAction::RemoveBubble);
    }

    #[test]
    fn tool_turn_empty_main_removed_when_completed_no_body_diag() {
        let a = decide_done_bubble_action(DoneBubbleDecisionInputs {
            body_and_reasoning_empty: true,
            end_reason_raw: Some("completed"),
            in_answer_body_lane: false,
            diag_chars: 0,
            has_hierarchical_or_tool: true,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, DoneBubbleAction::RemoveBubble);
    }

    #[test]
    fn fallback_timeline_dup_empty_removed() {
        let a = decide_done_bubble_action(DoneBubbleDecisionInputs {
            body_and_reasoning_empty: true,
            end_reason_raw: Some("fallback"),
            in_answer_body_lane: true,
            diag_chars: 0,
            has_hierarchical_or_tool: false,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, DoneBubbleAction::RemoveBubble);
    }

    #[test]
    fn unknown_reason_with_tool_history_fills_diagnostic() {
        let a = decide_done_bubble_action(DoneBubbleDecisionInputs {
            body_and_reasoning_empty: true,
            end_reason_raw: Some("unknown"),
            in_answer_body_lane: true,
            diag_chars: 0,
            has_hierarchical_or_tool: true,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, DoneBubbleAction::FillDiagnostic);
    }

    #[test]
    fn completed_plain_empty_removed() {
        let a = decide_done_bubble_action(DoneBubbleDecisionInputs {
            body_and_reasoning_empty: true,
            end_reason_raw: Some("completed"),
            in_answer_body_lane: true,
            diag_chars: 0,
            has_hierarchical_or_tool: false,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, DoneBubbleAction::RemoveBubble);
    }
}
