//! 空回复诊断、流式错误文案与「最终总结缺失」启发式。

use crabmate_sse_protocol::StreamEndReason;

use crate::i18n;
use crate::i18n::Locale;

pub(crate) fn build_empty_reply_with_diagnostic(
    loc: Locale,
    answer_phase_entered: bool,
    answer_delta_chars: usize,
    stream_end_reason: Option<&str>,
) -> String {
    // 兜底保护：已有终答阶段且收到不少增量，但缺失 `stream_ended`，
    // 这更像“收尾中断”而非“无回复”，避免误导用户。
    let reason_unknown = stream_end_reason
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_none_or(|s| s.eq_ignore_ascii_case("unknown"));
    if answer_phase_entered && answer_delta_chars > 0 && reason_unknown {
        let hint = i18n::stream_partial_finalize_missing_hint(loc);
        return format!(
            "{hint}\n\n{}",
            i18n::stream_empty_reply_diag_line(
                loc,
                stream_end_reason,
                answer_phase_entered,
                answer_delta_chars
            )
        );
    }
    let base = if !answer_phase_entered {
        i18n::stream_empty_reply_no_answer_phase(loc)
    } else if answer_delta_chars == 0 {
        i18n::stream_empty_reply_no_delta(loc)
    } else {
        i18n::stream_empty_reply(loc)
    };
    format!(
        "{base}\n\n{}",
        i18n::stream_empty_reply_diag_line(
            loc,
            stream_end_reason,
            answer_phase_entered,
            answer_delta_chars
        )
    )
}

pub(crate) fn build_stream_error_with_suggestion(raw: &str, loc: Locale) -> String {
    let msg = raw.trim();
    if msg.is_empty() {
        return raw.to_string();
    }
    let low = msg.to_lowercase();
    let (impact, hint) = if low.contains("llm_api_key_required")
        || low.contains("api key")
        || low.contains("unauthorized")
        || low.contains("401")
    {
        (
            i18n::stream_err_impact_api_key(loc),
            i18n::stream_err_hint_api_key(loc),
        )
    } else if low.contains("timeout") || low.contains("timed out") || low.contains("408") {
        (
            i18n::stream_err_impact_timeout(loc),
            i18n::stream_err_hint_timeout(loc),
        )
    } else {
        (
            i18n::stream_err_impact_generic(loc),
            i18n::stream_err_hint_generic(loc),
        )
    };
    i18n::format_error_three_part(loc, msg, impact, hint)
}

pub(crate) fn should_show_missing_final_summary_hint(
    end_reason: Option<&str>,
    in_answer_phase: bool,
    has_hierarchical_or_tool: bool,
    saw_final_response_timeline: bool,
) -> bool {
    // 须已收到 `assistant_answer_phase`：否则 `answer_delta_chars` 可能仅来自分层时间轴/子目标更新，
    // 与主气泡 `text` 无关，易误判「最终总结缺失」（见 issue：stream_ended=completed, answer_phase=false）。
    end_reason
        .and_then(|s| s.parse::<StreamEndReason>().ok())
        .is_some_and(|r| r == StreamEndReason::Completed)
        && in_answer_phase
        && has_hierarchical_or_tool
        && !saw_final_response_timeline
}
