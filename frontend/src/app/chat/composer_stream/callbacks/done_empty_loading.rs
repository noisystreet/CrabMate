//! `on_done` 中空 `loading` 尾泡的处置决策（纯函数，便于单测）。

use crabmate_sse_protocol::StreamEndReason;

use super::helpers::should_show_missing_final_summary_hint;

/// 进入「正文与 reasoning 皆空」分支后的输入。
#[derive(Clone, Copy)]
pub(super) struct EmptyLoadingTailInputs<'a> {
    pub end_reason_raw: Option<&'a str>,
    pub in_answer_body_lane: bool,
    pub diag_chars: usize,
    pub has_hierarchical_or_tool: bool,
    pub saw_final_response_timeline: bool,
}

/// 对空尾泡应采取的动作（不含 i18n 拼装，由调用方执行）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EmptyLoadingTailAction {
    RemoveBubble,
    FillMissingFinalHint,
    FillEmptyDiagnostic,
}

pub(super) fn decide_empty_loading_tail_action(
    inp: EmptyLoadingTailInputs<'_>,
) -> EmptyLoadingTailAction {
    let parsed = inp
        .end_reason_raw
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| s.parse::<StreamEndReason>().ok());

    let completed_with_visible_delta = parsed.is_some_and(|r| r == StreamEndReason::Completed)
        && inp.in_answer_body_lane
        && inp.diag_chars > 0;
    if completed_with_visible_delta {
        return EmptyLoadingTailAction::RemoveBubble;
    }

    let drop_empty_main_after_tool_like_turn = parsed.is_some_and(|r| {
        matches!(
            r,
            StreamEndReason::Completed
                | StreamEndReason::Fallback
                | StreamEndReason::Cancelled
                | StreamEndReason::NoOutput
        )
    }) && inp.has_hierarchical_or_tool
        && (!inp.in_answer_body_lane || inp.diag_chars == 0);
    if drop_empty_main_after_tool_like_turn {
        return EmptyLoadingTailAction::RemoveBubble;
    }

    let drop_redundant_empty_after_fallback_timeline = (parsed == Some(StreamEndReason::Fallback)
        || inp.saw_final_response_timeline)
        && inp.in_answer_body_lane
        && inp.diag_chars == 0;
    if drop_redundant_empty_after_fallback_timeline {
        return EmptyLoadingTailAction::RemoveBubble;
    }

    if should_show_missing_final_summary_hint(
        inp.end_reason_raw,
        inp.in_answer_body_lane,
        inp.has_hierarchical_or_tool,
        inp.saw_final_response_timeline,
    ) {
        EmptyLoadingTailAction::FillMissingFinalHint
    } else {
        EmptyLoadingTailAction::FillEmptyDiagnostic
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_with_answer_lane_and_diag_removes() {
        let a = decide_empty_loading_tail_action(EmptyLoadingTailInputs {
            end_reason_raw: Some("completed"),
            in_answer_body_lane: true,
            diag_chars: 3,
            has_hierarchical_or_tool: false,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, EmptyLoadingTailAction::RemoveBubble);
    }

    #[test]
    fn tool_turn_empty_main_removed_when_completed_no_body_diag() {
        let a = decide_empty_loading_tail_action(EmptyLoadingTailInputs {
            end_reason_raw: Some("completed"),
            in_answer_body_lane: false,
            diag_chars: 0,
            has_hierarchical_or_tool: true,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, EmptyLoadingTailAction::RemoveBubble);
    }

    #[test]
    fn fallback_timeline_dup_empty_removed() {
        let a = decide_empty_loading_tail_action(EmptyLoadingTailInputs {
            end_reason_raw: Some("fallback"),
            in_answer_body_lane: true,
            diag_chars: 0,
            has_hierarchical_or_tool: false,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, EmptyLoadingTailAction::RemoveBubble);
    }

    #[test]
    fn unknown_reason_with_tool_history_fills_diagnostic() {
        let a = decide_empty_loading_tail_action(EmptyLoadingTailInputs {
            end_reason_raw: Some("unknown"),
            in_answer_body_lane: true,
            diag_chars: 0,
            has_hierarchical_or_tool: true,
            saw_final_response_timeline: false,
        });
        assert_eq!(a, EmptyLoadingTailAction::FillEmptyDiagnostic);
    }
}
