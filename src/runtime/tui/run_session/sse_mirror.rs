//! 将 [`crate::sse::SsePayload`] 镜像进 [`super::turn_project`]，并可选追加中区「控制面」附录。
//!
//! 工具 / 时间线 / 思维迹已由 Turn 投影或流式 scratch 承接，**不再**写入 `[SSE 控制面]`，
//! 避免生成过程中出现调试感附录；附录仅保留 **错误**。

use std::sync::{Arc, Mutex};

use crate::runtime::tui::TuiLlmStreamScratchArc;
use crate::sse::{SseControlMirror, SsePayload};
use crate::text_util::truncate_chars_with_ellipsis;

use super::TuiModel;

pub(super) fn tui_sse_control_mirror(
    model: Arc<Mutex<TuiModel>>,
    llm_scratch: TuiLlmStreamScratchArc,
) -> SseControlMirror {
    Arc::new(move |p| {
        let scratch = llm_scratch.lock().unwrap_or_else(|e| e.into_inner());
        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
        g.turn_projection.apply_sse(&p, &scratch);
        drop(scratch);
        let Some(line) = format_sse_payload_one_line(&p) else {
            return;
        };
        if !g.control_plane_tail.is_empty() {
            g.control_plane_tail.push('\n');
        }
        g.control_plane_tail.push_str(&line);
        const MAX_LINES: usize = 48;
        let lines: Vec<&str> = g.control_plane_tail.lines().collect();
        if lines.len() > MAX_LINES {
            let skip = lines.len() - MAX_LINES;
            g.control_plane_tail = lines[skip..].join("\n");
        }
    })
}

/// 控制面附录一行；`None` = 仅驱动投影 / UI，不刷附录。
fn format_sse_payload_one_line(p: &SsePayload) -> Option<String> {
    match p {
        // 投影 / 流式 scratch / Modal 已承接 → 不刷附录（生成过程中勿出现 `[SSE 控制面]`）
        SsePayload::ToolCall { .. }
        | SsePayload::ToolResult { .. }
        | SsePayload::ToolRunning { .. }
        | SsePayload::ToolOutputChunk { .. }
        | SsePayload::ParsingToolCalls { .. }
        | SsePayload::TimelineLog { .. }
        | SsePayload::ThinkingTrace { .. }
        | SsePayload::AssistantAnswerPhase { .. }
        | SsePayload::TurnSegmentStart { .. }
        | SsePayload::TurnSegmentEnd { .. }
        | SsePayload::TurnToolPhaseEnd { .. }
        | SsePayload::ChatUiSeparator { .. }
        | SsePayload::WorkspaceChanged { .. }
        | SsePayload::PlanRequired { .. }
        | SsePayload::ConversationSaved { .. }
        | SsePayload::SseCapabilities { .. }
        | SsePayload::StreamEnded { .. }
        | SsePayload::ClarificationQuestionnaire { .. }
        | SsePayload::CommandApproval { .. } => None,
        SsePayload::Error(e) => Some(format!(
            "· 错误 {}",
            truncate_chars_with_ellipsis(&e.error, 200)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sse::ToolCallSummary;

    #[test]
    fn projected_tool_events_skip_control_plane_appendix() {
        assert!(
            format_sse_payload_one_line(&SsePayload::ToolCall {
                tool_call: ToolCallSummary {
                    name: "read_file".into(),
                    summary: "x".into(),
                    goal_id: None,
                    tool_call_id: Some("tc1".into()),
                    arguments_preview: None,
                    arguments: None,
                },
            })
            .is_none()
        );
        assert!(
            format_sse_payload_one_line(&SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            })
            .is_none()
        );
        assert!(
            format_sse_payload_one_line(&SsePayload::ToolRunning { tool_running: true }).is_none()
        );
    }

    #[test]
    fn thinking_and_timeline_skip_control_plane_during_generation() {
        assert!(
            format_sse_payload_one_line(&SsePayload::ThinkingTrace {
                trace: crate::sse::ThinkingTraceBody {
                    op: "reasoning_delta".into(),
                    node_id: None,
                    parent_id: None,
                    title: None,
                    chunk: Some("x".into()),
                    context_snapshot: None,
                },
            })
            .is_none()
        );
        assert!(
            format_sse_payload_one_line(&SsePayload::TimelineLog {
                log: crate::sse::protocol::TimelineLogBody {
                    kind: "orchestration_route".into(),
                    title: "hierarchical".into(),
                    detail: None,
                },
            })
            .is_none()
        );
    }

    #[test]
    fn error_still_appears_on_control_plane_appendix() {
        let line = format_sse_payload_one_line(&SsePayload::Error(crate::sse::SseErrorBody {
            error: "boom".into(),
            code: None,
            reason_code: None,
            turn_id: None,
            sub_phase: None,
        }))
        .expect("error line");
        assert!(line.contains("错误"), "{line}");
        assert!(line.contains("boom"), "{line}");
    }
}
