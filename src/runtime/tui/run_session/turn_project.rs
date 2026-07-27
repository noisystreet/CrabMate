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
    /// 已收到 [`SsePayload::TurnToolPhaseEnd`]（对齐 Web 工具批结束 → 终答区）。
    tool_phase_ended: bool,
    /// 定稿终答（`finalize_for_display` 从 scratch 固化）；流式中由 scratch 实时覆盖。
    final_answer_text: String,
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
                self.tool_phase_ended = true;
                self.capture_final_answer_from_scratch(scratch);
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

    /// 回合结束：关闭 open 段，并将工具批后 scratch 正文固化为终答。
    pub(super) fn finalize_for_display(&mut self, scratch: &TuiLlmStreamScratch) {
        self.flush_open_segment_from_scratch(scratch);
        close_open_commentary_segments(&mut self.turn);
        if self.tool_phase_ended || !self.turn.steps.is_empty() {
            // 有工具步但未收到 phase end 时仍尽量收下终答，避免只进 Message[] 丢行序
            if !self.tool_phase_ended && !self.turn.steps.is_empty() {
                self.tool_phase_ended = true;
            }
            self.capture_final_answer_from_scratch(scratch);
        }
    }

    /// Web v2 行 + open 旁白 + 工具批后终答（对齐 Tauri `turn-final-answer`；无元标签）。
    pub(super) fn format_projection_block(&self, scratch: Option<&TuiLlmStreamScratch>) -> String {
        let mut rows = project_turn_web_v2(&self.turn);
        if let Some(open) = streaming_commentary_block_text(&self.turn) {
            rows.push(ProjectedRow {
                kind: "assistant_commentary_open".into(),
                text: open,
                tool_name: None,
                tool_call_id: None,
            });
        }
        let final_text = self.resolved_final_answer(scratch);
        if !final_text.is_empty() {
            rows.push(ProjectedRow {
                kind: "assistant_answer".into(),
                text: final_text,
                tool_name: None,
                tool_call_id: None,
            });
        }
        if rows.is_empty() {
            return String::new();
        }
        format_projected_rows_for_tui(&rows)
    }

    /// 是否已有（或可预览）终答，供 flush 时跳过 Message[] 尾部双显。
    pub(super) fn has_final_answer(&self, scratch: Option<&TuiLlmStreamScratch>) -> bool {
        !self.resolved_final_answer(scratch).is_empty()
    }

    /// 流式 scratch 正文是否已由投影块承接（避免旁白/终答双显）。
    ///
    /// 工具相 / 已吸收 pending 旁白时隐藏 content；工具批结束后的终答进投影正文，亦隐藏。
    pub(super) fn should_hide_streaming_content(&self, scratch_content: &str) -> bool {
        let c = scratch_content.trim();
        if c.is_empty() {
            return false;
        }
        if self.turn.tool_phase_open {
            return true;
        }
        if self.tool_phase_ended && !commentary_text_covers_scratch(&self.turn, c) {
            return true;
        }
        if self.pending_stream_absorbed && !self.turn.steps.is_empty() {
            return commentary_text_covers_scratch(&self.turn, c);
        }
        commentary_text_covers_scratch(&self.turn, c)
    }

    fn resolved_final_answer(&self, scratch: Option<&TuiLlmStreamScratch>) -> String {
        if !self.tool_phase_ended {
            return String::new();
        }
        if let Some(s) = scratch {
            let c = s.content.trim();
            if !c.is_empty() && !commentary_text_covers_scratch(&self.turn, c) {
                return c.to_string();
            }
        }
        self.final_answer_text.trim().to_string()
    }

    fn capture_final_answer_from_scratch(&mut self, scratch: &TuiLlmStreamScratch) {
        let c = scratch.content.trim();
        if c.is_empty() || commentary_text_covers_scratch(&self.turn, c) {
            return;
        }
        self.final_answer_text = c.to_string();
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

    #[cfg(test)]
    pub(super) fn apply_turn_event_for_test(&mut self, event: TurnEvent) {
        TurnReducer.apply(&mut self.turn, event);
    }
}

pub(super) fn format_projected_rows_for_tui(rows: &[ProjectedRow]) -> String {
    let mut out = String::new();
    for row in rows {
        let text = truncate_chars_with_ellipsis(row.text.trim(), 4000);
        if text.is_empty() {
            continue;
        }
        match row.kind.as_str() {
            "tool" => {
                // 对齐 Tauri tool-card 一行摘要：▸ name  summary（无 [工具] 元标签）
                let name = row.tool_name.as_deref().unwrap_or("tool");
                out.push_str(&format!("▸ {name}  {text}\n\n"));
            }
            "assistant_timeline" => {
                out.push_str(&format!("· {text}\n\n"));
            }
            // 旁白 / 批说明 / 终答：与 Web 气泡一样直接出正文
            _ => {
                out.push_str(&text);
                out.push_str("\n\n");
            }
        }
    }
    out
}

fn commentary_text_covers_scratch(turn: &Turn, scratch: &str) -> bool {
    if let Some(open) = streaming_commentary_block_text(turn) {
        let o = open.trim();
        if !o.is_empty() && (scratch == o || scratch.starts_with(o) || o.starts_with(scratch)) {
            return true;
        }
    }
    for row in project_turn_web_v2(turn) {
        if row.kind != "assistant_commentary" && row.kind != "assistant_batch_narration" {
            continue;
        }
        let t = row.text.trim();
        if t.is_empty() {
            continue;
        }
        if scratch == t || scratch.starts_with(t) || t.starts_with(scratch) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tui::TuiLlmStreamScratch;
    use crate::sse::{ToolCallSummary, TurnSegmentEndBody, TurnSegmentStartBody};
    use crabmate_turn_layout::TurnEvent;

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
        let block = proj.format_projection_block(None);
        assert!(block.contains("先看一下 README。"), "{block}");
        assert!(block.contains("▸ read_file"), "{block}");
        let c = block.find("先看一下 README。").expect("commentary");
        let t = block.find("▸ read_file").expect("tool");
        assert!(c < t, "旁白正文须在工具行前: {block}");
        assert!(
            proj.should_hide_streaming_content("先看一下 README。"),
            "tool-phase / absorbed commentary must hide scratch duplicate"
        );
    }

    #[test]
    fn post_tool_final_answer_lands_in_projection_not_streaming_tail() {
        let mut proj = TuiTurnProjection::default();
        let mut scratch = TuiLlmStreamScratch {
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
        proj.apply_sse(
            &SsePayload::TurnToolPhaseEnd {
                turn_tool_phase_end: true,
            },
            &scratch,
        );
        scratch.content = "总结如下。".into();
        let block = proj.format_projection_block(Some(&scratch));
        assert!(block.contains("▸ read_file"), "{block}");
        let tool_at = block.find("▸ read_file").expect("tool");
        let final_at = block.find("总结如下。").expect("终答 missing");
        assert!(final_at > tool_at, "终答须在工具之后: {block}");
        assert!(
            proj.should_hide_streaming_content("总结如下。"),
            "final answer must not also stream as [assistant] tail"
        );
        proj.finalize_for_display(&scratch);
        let committed = proj.format_projection_block(None);
        assert!(
            committed.contains("总结如下。") && proj.has_final_answer(None),
            "finalize must keep 终答: {committed}"
        );
    }

    /// Phase 6：模拟 SSE 镜像时序（旁白吸收 → 工具 → phase end → 终答），锁定行序金样。
    #[test]
    fn sse_sequence_projects_commentary_tool_final_in_order() {
        let mut proj = TuiTurnProjection::default();
        let mut scratch = TuiLlmStreamScratch {
            content: "我先读 README。".into(),
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
                    tool_call_id: Some("tc-readme".into()),
                    arguments_preview: Some("{\"path\":\"README.md\"}".into()),
                    arguments: None,
                },
            },
            &scratch,
        );
        proj.apply_sse(
            &SsePayload::TurnToolPhaseEnd {
                turn_tool_phase_end: true,
            },
            &scratch,
        );
        scratch.content = "目录如下。".into();
        proj.finalize_for_display(&scratch);
        let block = proj.format_projection_block(None);
        let c = block.find("我先读 README。").expect("旁白");
        let t = block.find("▸ read_file").expect("工具");
        let a = block.find("目录如下。").expect("终答");
        assert!(c < t && t < a, "SSE 时序投影须旁白→工具→终答:\n{block}");
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

    #[test]
    fn golden_web_v2_row_order_preserved_in_tui_projection_block() {
        use serde::Deserialize;
        use std::path::PathBuf;

        #[derive(Debug, Deserialize)]
        struct GoldenCase {
            id: String,
            events: Vec<TurnEvent>,
        }

        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/turn_project_golden.jsonl");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let case: GoldenCase = serde_json::from_str(t).unwrap_or_else(|e| {
                panic!("{}:{}: {e}\n{t}", path.display(), line_no + 1);
            });
            let mut proj = TuiTurnProjection::default();
            for ev in case.events {
                proj.apply_turn_event_for_test(ev);
            }
            proj.finalize_for_display(&TuiLlmStreamScratch::default());
            let web_rows = project_turn_web_v2(proj.turn_ref());
            let block = proj.format_projection_block(None);
            assert!(
                !block.is_empty() || web_rows.is_empty(),
                "case {}: empty block for non-empty rows in {block}",
                case.id
            );
            assert!(
                !block.contains("[Turn 投影]") && !block.contains("[旁白]"),
                "case {}: must not use meta labels: {block}",
                case.id
            );
            let mut search_from = 0usize;
            for (i, row) in web_rows.iter().enumerate() {
                let needle = row.text.trim();
                if needle.is_empty() {
                    continue;
                }
                let Some(rel) = block[search_from..].find(needle) else {
                    panic!(
                        "case {}: row[{i}] kind={} text={needle:?} not found in order after {search_from} in:\n{block}",
                        case.id, row.kind
                    );
                };
                search_from += rel + needle.len();
            }
        }
    }
}
