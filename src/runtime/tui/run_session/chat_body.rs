//! 中区聊天正文合成：定稿 transcript + 本轮投影 + 控制面附录 + 流式尾。

use crate::runtime::message_display::{
    assistant_markdown_source_for_display, assistant_raw_markdown_body_from_parts,
};
use crate::runtime::tui::TuiLlmStreamScratch;
use crate::text_util::truncate_chars_with_ellipsis;

use super::turn_project::TuiTurnProjection;

/// 流式尾挂：仅当投影**尚未**拥有 content lane 时附加 `[assistant]\n{body}`。
/// open 段 / 工具相 / 旁白 / 终答由投影（含 live catch-up）承接，避免双显与藏短。
pub(super) fn append_tui_streaming_tail(
    transcript: &str,
    scratch: &TuiLlmStreamScratch,
    projection: &TuiTurnProjection,
) -> String {
    let r = scratch.reasoning.trim();
    let c = scratch.content.trim();
    let hide_content = projection.owns_streaming_content_lane(scratch);
    let body = streaming_assistant_body_matching_transcript(r, c, hide_content);
    if body.is_empty() {
        return transcript.to_string();
    }
    let mut out = String::from(transcript);
    // transcript 通常已以 `\n\n` 结尾；勿再多插空行，否则相对终态会下移。
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str("[assistant]\n");
    out.push_str(body.as_str());
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push('\n');
    out
}

/// 与终态 `assistant_markdown_source_for_message` 同源组装，截断上限对齐 transcript。
fn streaming_assistant_body_matching_transcript(
    reasoning: &str,
    content: &str,
    hide_content: bool,
) -> String {
    let c = if hide_content { "" } else { content };
    let raw = assistant_raw_markdown_body_from_parts(reasoning, c);
    let t = assistant_markdown_source_for_display(&raw);
    let trimmed = t.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        truncate_chars_with_ellipsis(trimmed, 12_000)
    }
}

/// 中区聊天正文唯一合成入口：定稿 transcript + 本轮投影 + 控制面附录 + 流式尾。
pub(super) fn build_tui_chat_body(
    transcript: &str,
    turn_projection: &TuiTurnProjection,
    control_plane_tail: &str,
    scratch: &TuiLlmStreamScratch,
) -> String {
    let mut out = transcript.to_string();
    let projection = turn_projection.format_projection_block(Some(scratch));
    append_chat_section_preserving_blank(&mut out, projection.as_str());
    if !control_plane_tail.is_empty() {
        let mut ctrl = String::from("[SSE 控制面]\n");
        ctrl.push_str(control_plane_tail);
        append_chat_section_preserving_blank(&mut out, ctrl.as_str());
    }
    append_tui_streaming_tail(out.as_str(), scratch, turn_projection)
}

/// 在已有 transcript 后追加一节。若末尾已是空行（`\n\n`），不再多插，避免流式相对定稿下移一行。
fn append_chat_section_preserving_blank(out: &mut String, section: &str) {
    if section.is_empty() {
        return;
    }
    if !out.is_empty() {
        if out.ends_with("\n\n") {
            // 已有块间距
        } else if out.ends_with('\n') {
            out.push('\n');
        } else {
            out.push_str("\n\n");
        }
    }
    out.push_str(section);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tui::TuiLlmStreamScratch;

    #[test]
    fn build_tui_chat_body_matches_prepare_and_scrollbar_path() {
        let transcript = "[user]\nhi\n\n";
        let scratch = TuiLlmStreamScratch {
            content: "你好".into(),
            ..Default::default()
        };
        let projection = TuiTurnProjection::default();
        let body = build_tui_chat_body(transcript, &projection, "", &scratch);
        let via_tail = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert_eq!(body, via_tail);
        assert!(body.contains("[assistant]\n你好"), "{body}");
        let with_ctrl = build_tui_chat_body(transcript, &projection, "err line", &scratch);
        assert!(with_ctrl.contains("[SSE 控制面]\nerr line"), "{with_ctrl}");
    }

    #[test]
    fn projection_join_does_not_add_extra_blank_before_assistant() {
        let transcript = "[user]\nhi\n\n";
        let scratch = TuiLlmStreamScratch {
            content: "先看一下 README。".into(),
            ..Default::default()
        };
        let mut projection = TuiTurnProjection::default();
        projection.apply_sse(
            &crate::sse::SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            },
            &scratch,
        );
        let body = build_tui_chat_body(transcript, &projection, "", &scratch);
        assert!(
            body.starts_with("[user]\nhi\n\n[assistant]\n"),
            "projection must sit flush like streaming tail / final: {body:?}"
        );
        assert!(
            !body.contains("[user]\nhi\n\n\n[assistant]"),
            "extra blank shifts [assistant] down during generate: {body:?}"
        );
    }

    #[test]
    fn timeline_then_stream_does_not_add_extra_blank_after_user() {
        let transcript = "[user]\nhi\n\n";
        let scratch = TuiLlmStreamScratch {
            content: "正文回答".into(),
            ..Default::default()
        };
        let mut projection = TuiTurnProjection::default();
        projection.apply_sse(
            &crate::sse::SsePayload::TimelineLog {
                log: crate::sse::protocol::TimelineLogBody {
                    kind: "intent_analysis".into(),
                    title: "直接执行".into(),
                    detail: None,
                },
            },
            &scratch,
        );
        let body = build_tui_chat_body(transcript, &projection, "", &scratch);
        assert!(
            !body.contains("[user]\nhi\n\n\n·"),
            "extra blank before timeline pushes later [assistant]: {body:?}"
        );
        assert!(
            body.contains("[user]\nhi\n\n·") || body.contains("[user]\nhi\n\n[assistant]"),
            "{body:?}"
        );
    }

    #[test]
    fn streaming_tail_keeps_assistant_header() {
        let transcript = "[user]\nhi\n\n";
        let scratch = TuiLlmStreamScratch {
            reasoning: String::new(),
            content: "你好，世界".into(),
        };
        let projection = TuiTurnProjection::default();
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert!(
            out.starts_with("[user]\nhi\n\n[assistant]\n"),
            "stream must keep [assistant] like final transcript: {out:?}"
        );
        assert!(out.contains("你好，世界"), "stream body missing: {out:?}");
        assert!(
            out.ends_with("\n\n"),
            "stream must end with blank line like messages_to_transcript: {out:?}"
        );
        assert!(
            !out.contains("[user]\nhi\n\n\n[assistant]"),
            "extra blank before [assistant] shifts text vs final: {out:?}"
        );
    }

    #[test]
    fn streaming_tail_still_shows_when_only_timeline_in_projection() {
        let transcript = "[user]\nhi\n\n";
        let scratch = TuiLlmStreamScratch {
            content: "正文回答".into(),
            ..Default::default()
        };
        let mut projection = TuiTurnProjection::default();
        projection.apply_sse(
            &crate::sse::SsePayload::TimelineLog {
                log: crate::sse::protocol::TimelineLogBody {
                    kind: "intent_analysis".into(),
                    title: "直接执行".into(),
                    detail: None,
                },
            },
            &scratch,
        );
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert!(
            out.contains("正文回答"),
            "timeline-only projection must not suppress stream body: {out:?}"
        );
        assert!(out.contains("[assistant]\n正文回答"), "{out:?}");
    }

    #[test]
    fn streaming_tail_suppressed_when_projection_owns_content() {
        let transcript = "[user]\nhi\n\n";
        let scratch = TuiLlmStreamScratch {
            content: "先看一下 README。".into(),
            ..Default::default()
        };
        let mut projection = TuiTurnProjection::default();
        projection.apply_sse(
            &crate::sse::SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            },
            &scratch,
        );
        let out = append_tui_streaming_tail(transcript, &scratch, &projection);
        assert_eq!(
            out, transcript,
            "projection commentary must not also stream: {out:?}"
        );
        let block = projection.format_projection_block(Some(&scratch));
        assert!(
            block.contains("[assistant]\n先看一下 README。"),
            "projection must keep [assistant] so the label does not vanish: {block}"
        );
    }
}
