//! 将 [`crate::sse::SsePayload`] 格式化为 TUI 中区「控制面」附录（与 Web SSE 控制面对齐）。

use std::sync::{Arc, Mutex};

use crate::sse::{SseControlMirror, SsePayload};
use crate::text_util::truncate_chars_with_ellipsis;

use super::TuiModel;

pub(super) fn tui_sse_control_mirror(model: Arc<Mutex<TuiModel>>) -> SseControlMirror {
    Arc::new(move |p| {
        let Some(line) = format_sse_payload_one_line(&p) else {
            return;
        };
        let mut g = model.lock().unwrap_or_else(|e| e.into_inner());
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

fn format_sse_payload_one_line(p: &SsePayload) -> Option<String> {
    match p {
        SsePayload::ToolCall { tool_call } => {
            let prev = tool_call
                .arguments_preview
                .as_deref()
                .map(|s| truncate_chars_with_ellipsis(s, 120));
            Some(format!(
                "· 工具 · {} {}",
                tool_call.name,
                prev.unwrap_or_default()
            ))
        }
        SsePayload::ToolResult { tool_result } => Some(format!(
            "· 结果 · {} {}",
            tool_result.name,
            if tool_result.ok == Some(false) {
                "失败"
            } else {
                "完成"
            }
        )),
        SsePayload::ToolRunning { tool_running } => Some(format!(
            "· 工具执行中… {}",
            if *tool_running { "开始" } else { "结束" }
        )),
        SsePayload::ToolOutputChunk { tool_output_chunk } => Some(format!(
            "· 工具输出 seq={} {}",
            tool_output_chunk.seq,
            truncate_chars_with_ellipsis(&tool_output_chunk.chunk, 120)
        )),
        SsePayload::ParsingToolCalls { parsing_tool_calls } => Some(format!(
            "· 解析 tool_calls … {}",
            if *parsing_tool_calls {
                "进行中"
            } else {
                "结束"
            }
        )),
        SsePayload::TimelineLog { log } => Some(format!(
            "· {} {}",
            log.kind,
            truncate_chars_with_ellipsis(&log.title, 200)
        )),
        SsePayload::ThinkingTrace { .. } => Some("· 思维迹".to_string()),
        SsePayload::AssistantAnswerPhase { .. } => None,
        SsePayload::TurnSegmentStart { .. }
        | SsePayload::TurnSegmentEnd { .. }
        | SsePayload::TurnToolPhaseEnd { .. } => None,
        SsePayload::ChatUiSeparator { .. } => None,
        SsePayload::WorkspaceChanged { .. } => None,
        SsePayload::PlanRequired { .. } => None,
        SsePayload::ConversationSaved { .. } => None,
        SsePayload::SseCapabilities { .. } => None,
        SsePayload::StreamEnded { .. } => None,
        SsePayload::ClarificationQuestionnaire { .. } => None,
        SsePayload::CommandApproval { .. } => None,
        SsePayload::Error(e) => Some(format!(
            "· 错误 {}",
            truncate_chars_with_ellipsis(&e.error, 200)
        )),
    }
}
