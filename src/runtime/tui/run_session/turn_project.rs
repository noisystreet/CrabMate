//! TUI 单轮 canonical 投影：消费 [`crabmate_turn_layout`]，与 Web/Tauri `project_turn_web_v2` 同行序。

use crabmate_turn_layout::{
    PENDING_STREAM_COMMENTARY_SEGMENT_ID, ProjectedRow, SegmentKind, Turn, TurnEvent, TurnReducer,
    close_open_commentary_segments, project_turn_web_v2, streaming_commentary_block_text,
};

use crate::runtime::tui::TuiLlmStreamScratch;
use crate::sse::SsePayload;
use crate::text_util::truncate_chars_with_ellipsis;

/// 本轮 TUI 侧 Turn reducer 状态（与 Web `TurnCanonicalState` 同源事件语义）。
#[derive(Debug, Default)]
pub(super) struct TuiTurnProjection {
    turn: Turn,
    /// [`SsePayload::TurnSegmentStart`] 时流式 scratch `content` 字节游标；段结束时切片写入 SegmentDelta。
    scratch_cursor_at_segment_start: Option<usize>,
    /// 是否已把工具前整段 scratch 吸收进 `pending-stream-commentary`（形态 B）。
    pending_stream_absorbed: bool,
    open_segment_id: Option<String>,
}

impl TuiTurnProjection {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    #[cfg(test)]
    pub(super) fn turn_ref(&self) -> &Turn {
        &self.turn
    }

    /// 应用控制面 SSE；在工具声明前把流式 scratch 旁白迁入 pending（对齐 Web demote/peel）。
    pub(super) fn apply_sse(&mut self, payload: &SsePayload, scratch: &TuiLlmStreamScratch) {
        match payload {
            SsePayload::TurnSegmentStart { start } => {
                self.flush_open_segment_from_scratch(scratch);
                let kind = match start.kind.as_str() {
                    "answer" => SegmentKind::Answer,
                    _ => SegmentKind::Commentary,
                };
                TurnReducer.apply(
                    &mut self.turn,
                    TurnEvent::SegmentStart {
                        segment_id: start.segment_id.clone(),
                        kind,
                        before_tool_call_id: start.before_tool_call_id.clone(),
                    },
                );
                self.open_segment_id = Some(start.segment_id.clone());
                self.scratch_cursor_at_segment_start = Some(scratch.content.len());
            }
            SsePayload::TurnSegmentEnd { end } => {
                self.flush_open_segment_from_scratch(scratch);
                TurnReducer.apply(
                    &mut self.turn,
                    TurnEvent::SegmentEnd {
                        segment_id: end.segment_id.clone(),
                    },
                );
                if self.open_segment_id.as_deref() == Some(end.segment_id.as_str()) {
                    self.open_segment_id = None;
                    self.scratch_cursor_at_segment_start = None;
                }
            }
            SsePayload::TurnToolPhaseEnd { .. } => {
                self.flush_open_segment_from_scratch(scratch);
                close_open_commentary_segments(&mut self.turn);
                TurnReducer.apply(&mut self.turn, TurnEvent::ToolPhaseEnd);
            }
            SsePayload::ToolCall { tool_call } => {
                self.absorb_pre_tool_scratch_if_needed(scratch);
                self.flush_open_segment_from_scratch(scratch);
                let tool_call_id = tool_call
                    .tool_call_id
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or_else(|| format!("tui-anon-{}", self.turn.steps.len()));
                let summary = if tool_call.summary.trim().is_empty() {
                    tool_call.arguments_preview.clone().unwrap_or_default()
                } else {
                    tool_call.summary.clone()
                };
                TurnReducer.apply(
                    &mut self.turn,
                    TurnEvent::ToolCall {
                        tool_call_id,
                        name: tool_call.name.clone(),
                        summary,
                    },
                );
            }
            SsePayload::TimelineLog { log } => {
                let kind = log.kind.as_str();
                if matches!(
                    kind,
                    "intent_analysis" | "approval_decision" | "tool_result_summary"
                ) && !log.title.trim().is_empty()
                {
                    TurnReducer.apply(
                        &mut self.turn,
                        TurnEvent::TimelineAssistant {
                            text: log.title.clone(),
                        },
                    );
                }
            }
            SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            } => {
                self.absorb_pre_tool_scratch_if_needed(scratch);
            }
            _ => {}
        }
    }

    /// 回合结束：关闭 open 段，供最终投影。
    pub(super) fn finalize_for_display(&mut self, scratch: &TuiLlmStreamScratch) {
        self.flush_open_segment_from_scratch(scratch);
        close_open_commentary_segments(&mut self.turn);
    }

    /// Web v2 行 + 仍 open 的旁白预览（与 Tauri loading overlay 语义对齐）。
    pub(super) fn format_projection_block(&self) -> String {
        let mut rows = project_turn_web_v2(&self.turn);
        if let Some(open) = streaming_commentary_block_text(&self.turn) {
            rows.push(ProjectedRow {
                kind: "assistant_commentary_open".into(),
                text: open,
                tool_name: None,
                tool_call_id: None,
            });
        }
        if rows.is_empty() {
            return String::new();
        }
        format_projected_rows_for_tui(&rows)
    }

    fn absorb_pre_tool_scratch_if_needed(&mut self, scratch: &TuiLlmStreamScratch) {
        if self.pending_stream_absorbed || self.turn.tool_phase_open {
            return;
        }
        // 已有锚定旁白段时走 segment 路径，勿把整段 scratch 再吸入 pending。
        if self
            .turn
            .segments
            .iter()
            .any(|s| s.kind == SegmentKind::Commentary && !s.text.trim().is_empty())
            || self.turn.steps.iter().any(|s| {
                s.before_commentary
                    .as_ref()
                    .is_some_and(|t| !t.trim().is_empty())
            })
        {
            return;
        }
        let text = scratch.content.trim();
        if text.is_empty() {
            return;
        }
        TurnReducer.apply(
            &mut self.turn,
            TurnEvent::SegmentStart {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                kind: SegmentKind::Commentary,
                before_tool_call_id: None,
            },
        );
        TurnReducer.apply(
            &mut self.turn,
            TurnEvent::SegmentDelta {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.into(),
                delta: text.to_string(),
            },
        );
        self.pending_stream_absorbed = true;
    }

    fn flush_open_segment_from_scratch(&mut self, scratch: &TuiLlmStreamScratch) {
        let (Some(seg_id), Some(cursor)) = (
            self.open_segment_id.clone(),
            self.scratch_cursor_at_segment_start,
        ) else {
            return;
        };
        let content = scratch.content.as_str();
        if cursor >= content.len() {
            return;
        }
        let slice = &content[cursor..];
        if slice.is_empty() {
            return;
        }
        TurnReducer.apply(
            &mut self.turn,
            TurnEvent::SegmentDelta {
                segment_id: seg_id,
                delta: slice.to_string(),
            },
        );
        self.scratch_cursor_at_segment_start = Some(content.len());
    }
}

pub(super) fn format_projected_rows_for_tui(rows: &[ProjectedRow]) -> String {
    let mut out = String::from("[Turn 投影]\n");
    for row in rows {
        let label = match row.kind.as_str() {
            "assistant_timeline" => "时间线",
            "assistant_commentary" | "assistant_commentary_open" => "旁白",
            "assistant_batch_narration" => "批说明",
            "assistant_answer" => "终答",
            "tool" => "工具",
            other => other,
        };
        let text = truncate_chars_with_ellipsis(row.text.trim(), 4000);
        if text.is_empty() {
            continue;
        }
        if let Some(ref name) = row.tool_name {
            out.push_str(&format!("[{label} · {name}]\n{text}\n\n"));
        } else {
            out.push_str(&format!("[{label}]\n{text}\n\n"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tui::TuiLlmStreamScratch;
    use crate::sse::{ToolCallSummary, TurnSegmentEndBody, TurnSegmentStartBody};

    #[test]
    fn form_b_scratch_absorbed_before_tool_projects_commentary_then_tool() {
        let mut proj = TuiTurnProjection::default();
        let scratch = TuiLlmStreamScratch {
            content: "先看一下 README。".into(),
            ..Default::default()
        };
        proj.apply_sse(
            &SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            },
            &scratch,
        );
        proj.apply_sse(
            &SsePayload::ToolCall {
                tool_call: ToolCallSummary {
                    name: "read_file".into(),
                    summary: "README.md".into(),
                    goal_id: None,
                    tool_call_id: Some("tc1".into()),
                    arguments_preview: None,
                    arguments: None,
                },
            },
            &scratch,
        );
        let rows = project_turn_web_v2(proj.turn_ref());
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].kind, "assistant_commentary");
        assert!(rows[0].text.contains("README"));
        assert_eq!(rows[1].kind, "tool");
        assert_eq!(rows[1].tool_name.as_deref(), Some("read_file"));
        let block = proj.format_projection_block();
        assert!(block.contains("[旁白]"), "{block}");
        assert!(block.contains("[工具 · read_file]"), "{block}");
    }

    #[test]
    fn segment_start_end_captures_scratch_slice() {
        let mut proj = TuiTurnProjection::default();
        let mut scratch = TuiLlmStreamScratch {
            content: "忽略前缀。".into(),
            ..Default::default()
        };
        let cursor = scratch.content.len();
        proj.apply_sse(
            &SsePayload::TurnSegmentStart {
                start: TurnSegmentStartBody {
                    segment_id: "seg-before-tc1".into(),
                    kind: "commentary".into(),
                    before_tool_call_id: Some("tc1".into()),
                },
            },
            &scratch,
        );
        scratch.content.push_str("旁白正文。");
        assert_eq!(proj.scratch_cursor_at_segment_start, Some(cursor));
        proj.apply_sse(
            &SsePayload::TurnSegmentEnd {
                end: TurnSegmentEndBody {
                    segment_id: "seg-before-tc1".into(),
                },
            },
            &scratch,
        );
        proj.apply_sse(
            &SsePayload::ToolCall {
                tool_call: ToolCallSummary {
                    name: "read_file".into(),
                    summary: "f".into(),
                    goal_id: None,
                    tool_call_id: Some("tc1".into()),
                    arguments_preview: None,
                    arguments: None,
                },
            },
            &scratch,
        );
        let rows = project_turn_web_v2(proj.turn_ref());
        assert_eq!(rows[0].kind, "assistant_commentary");
        assert_eq!(rows[0].text, "旁白正文。");
    }
}
