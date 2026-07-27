//! 将 [`crate::sse::SsePayload`] 镜像进 [`super::turn_project`]，并可选追加中区「控制面」附录。
//!
//! **Phase 3**：已由 Turn 投影承接的工具/解析事件**不再**写入 `[SSE 控制面]`，避免与
//! `[Turn 投影]` 双列；附录仅保留错误、思维迹，以及未进投影的 `timeline_log`。

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
        // 已由 turn_projection / 底栏 tool_running_hook 承接 → 不刷附录
        SsePayload::ToolCall { .. }
        | SsePayload::ToolResult { .. }
        | SsePayload::ToolRunning { .. }
        | SsePayload::ToolOutputChunk { .. }
        | SsePayload::ParsingToolCalls { .. } => None,
        SsePayload::TimelineLog { log } => {
            // 已写入投影时间线行的 kind 不再附录
            if matches!(
                log.kind.as_str(),
                "intent_analysis" | "approval_decision" | "tool_result_summary"
            ) {
                return None;
            }
            Some(format!(
                "· {} {}",
                log.kind,
                truncate_chars_with_ellipsis(&log.title, 200)
            ))
        }
        SsePayload::ThinkingTrace { .. } => Some("· 思维迹".to_string()),
        SsePayload::AssistantAnswerPhase { .. }
        | SsePayload::TurnSegmentStart { .. }
        | SsePayload::TurnSegmentEnd { .. }
        | SsePayload::TurnToolPhaseEnd { .. } => None,
        SsePayload::ChatUiSeparator { .. } => None,
        SsePayload::WorkspaceChanged { .. } => None,
        SsePayload::PlanRequired { .. } => None,
        SsePayload::ConversationSaved { .. } => None,
        SsePayload::SseCapabilities { .. } => None,
        SsePayload::StreamEnded { .. } => None,
        // 审批 / 澄清走 Modal，不附录
        SsePayload::ClarificationQuestionnaire { .. } => None,
        SsePayload::CommandApproval { .. } => None,
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
