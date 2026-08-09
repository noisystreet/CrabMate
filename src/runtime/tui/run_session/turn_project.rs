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
                // 关闭 turn 段后必须清本地 open 游标，否则后续终答会经 live catch-up /
                // finalize flush 再写进旁白，造成「终答出现在工具前」的双显。
                self.clear_open_segment_cursor();
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
                if matches!(kind, "approval_decision" | "tool_result_summary")
                    && !log.title.trim().is_empty()
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
        if !self.tool_phase_ended {
            self.flush_open_segment_from_scratch(scratch);
        }
        close_open_commentary_segments(&mut self.turn);
        self.clear_open_segment_cursor();
        if self.tool_phase_ended || !self.turn.steps.is_empty() {
            // 有工具步但未收到 phase end 时仍尽量收下终答，避免只进 Message[] 丢行序
            if !self.tool_phase_ended && !self.turn.steps.is_empty() {
                self.tool_phase_ended = true;
            }
            self.capture_final_answer_from_scratch(scratch);
        }
    }

    /// 是否具备可写入定稿的 turn 布局正文（旁白 / 工具步 / 终答）。
    ///
    /// 仅有 `assistant_timeline`（如 tool_result_summary）时返回 false：定稿应走 Message[]，
    /// 避免「投影非空 → 跳过全部 assistant」丢掉真正回答。
    pub(super) fn has_flushable_turn_layout(&self) -> bool {
        if !self.turn.steps.is_empty() {
            return true;
        }
        if !self.final_answer_text.trim().is_empty() {
            return true;
        }
        if self.turn.segments.iter().any(|s| !s.text.trim().is_empty()) {
            return true;
        }
        if streaming_commentary_block_text(&self.turn).is_some_and(|t| !t.trim().is_empty()) {
            return true;
        }
        false
    }

    /// Web v2 行 + open 旁白（含未 flush 的 scratch 增量）+ 工具批后终答。
    pub(super) fn format_projection_block(&self, scratch: Option<&TuiLlmStreamScratch>) -> String {
        let mut rows = project_turn_web_v2(&self.turn);
        // 工具批结束后 content lane 归终答；勿再把累积 scratch 挂成 open 旁白。
        if !self.tool_phase_ended
            && let Some(open) = self.live_open_commentary_text(scratch)
        {
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

    /// 本轮 **content 正文 lane** 是否已由投影承接。
    ///
    /// 为 true 时流式尾不再挂 `scratch.content`（仍可由投影 live catch-up / 终答行展示）。
    /// 仅 timeline、或尚无旁白/工具相/终答的纯问答，返回 false，允许流式尾跟字。
    pub(super) fn owns_streaming_content_lane(&self, scratch: &TuiLlmStreamScratch) -> bool {
        if self.open_segment_id.is_some() {
            return true;
        }
        if self.turn.tool_phase_open || self.tool_phase_ended {
            return true;
        }
        if !self.final_answer_text.trim().is_empty() {
            return true;
        }
        if self
            .live_open_commentary_text(Some(scratch))
            .is_some_and(|t| !t.trim().is_empty())
        {
            return true;
        }
        project_turn_web_v2(&self.turn).iter().any(|row| {
            matches!(
                row.kind.as_str(),
                "assistant_commentary" | "assistant_batch_narration"
            ) && !row.text.trim().is_empty()
        })
    }

    /// Message[] 中的纯文本 assistant 是否已由投影旁白/终答承接（flush 时避免双显）。
    pub(super) fn covers_plain_assistant_body(&self, body: &str) -> bool {
        let b = body.trim();
        if b.is_empty() {
            return true;
        }
        let final_t = self.final_answer_text.trim();
        if !final_t.is_empty() && projection_text_covers(b, final_t) {
            return true;
        }
        if projection_rows_cover_assistant_body(b, &self.turn) {
            return true;
        }
        turn_segments_cover_assistant_body(b, &self.turn)
    }

    /// open 旁白 + 自 `scratch_cursor` 起尚未写入 reducer 的 scratch 切片（供绘制即时跟底）。
    fn live_open_commentary_text(&self, scratch: Option<&TuiLlmStreamScratch>) -> Option<String> {
        let mut open = streaming_commentary_block_text(&self.turn).unwrap_or_default();
        if self.open_segment_id.is_some()
            && let (Some(s), Some(cursor)) = (scratch, self.scratch_cursor_at_segment_start)
            && cursor < s.content.len()
        {
            open.push_str(&s.content[cursor..]);
        }
        if open.trim().is_empty() {
            None
        } else {
            Some(open)
        }
    }

    fn commentary_covers_scratch(
        &self,
        scratch_content: &str,
        scratch: Option<&TuiLlmStreamScratch>,
    ) -> bool {
        let c = scratch_content.trim();
        if let Some(open) = self.live_open_commentary_text(scratch) {
            let o = open.trim();
            if !o.is_empty() && projection_text_covers(c, o) {
                return true;
            }
        }
        for row in project_turn_web_v2(&self.turn) {
            if row.kind != "assistant_commentary" && row.kind != "assistant_batch_narration" {
                continue;
            }
            let t = row.text.trim();
            if t.is_empty() {
                continue;
            }
            if projection_text_covers(c, t) {
                return true;
            }
        }
        false
    }

    fn clear_open_segment_cursor(&mut self) {
        self.open_segment_id = None;
        self.scratch_cursor_at_segment_start = None;
    }

    fn resolved_final_answer(&self, scratch: Option<&TuiLlmStreamScratch>) -> String {
        if !self.tool_phase_ended {
            return String::new();
        }
        if let Some(s) = scratch {
            let c = s.content.trim();
            if let Some(final_part) = self.scratch_final_suffix(c)
                && !final_part.is_empty()
            {
                return final_part.to_string();
            }
        }
        self.final_answer_text.trim().to_string()
    }

    fn capture_final_answer_from_scratch(&mut self, scratch: &TuiLlmStreamScratch) {
        let c = scratch.content.trim();
        let Some(final_part) = self.scratch_final_suffix(c) else {
            return;
        };
        if final_part.is_empty() {
            return;
        }
        self.final_answer_text = final_part.to_string();
    }

    /// 工具批后 scratch 常累积「旁白+终答」；终答取旁白前缀之后的后缀。
    fn scratch_final_suffix<'a>(&self, scratch_content: &'a str) -> Option<&'a str> {
        let c = scratch_content.trim();
        if c.is_empty() {
            return None;
        }
        for row in project_turn_web_v2(&self.turn) {
            if row.kind != "assistant_commentary" && row.kind != "assistant_batch_narration" {
                continue;
            }
            let t = row.text.trim();
            if t.is_empty() {
                continue;
            }
            if let Some(rest) = c.strip_prefix(t) {
                let rest = rest.trim_start();
                if !rest.is_empty() {
                    return Some(rest);
                }
                // 整段仍是旁白，尚无终答
                return None;
            }
        }
        if self.commentary_covers_scratch(c, None) {
            return None;
        }
        Some(c)
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
        // 工具批已结束后的增量属终答 lane，不得再写入 commentary 段。
        if self.tool_phase_ended {
            return;
        }
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
            // 旁白 / 批说明 / 终答：带 [assistant]，与流式尾 / Message[] 定稿一致，避免标签闪没。
            _ => {
                out.push_str("[assistant]\n");
                out.push_str(&text);
                out.push_str("\n\n");
            }
        }
    }
    out
}

fn projection_rows_cover_assistant_body(body: &str, turn: &Turn) -> bool {
    for row in project_turn_web_v2(turn) {
        if !matches!(
            row.kind.as_str(),
            "assistant_commentary" | "assistant_batch_narration" | "assistant_answer"
        ) {
            continue;
        }
        let t = row.text.trim();
        if !t.is_empty() && projection_text_covers(body, t) {
            return true;
        }
    }
    false
}

fn turn_segments_cover_assistant_body(body: &str, turn: &Turn) -> bool {
    for seg in &turn.segments {
        let t = seg.text.trim();
        if !t.is_empty() && projection_text_covers(body, t) {
            return true;
        }
    }
    false
}

/// 投影旁白是否已覆盖（或领先于）scratch：相等、投影为前缀扩张、或 scratch 仍是投影前缀。
///
/// **注意**：`scratch.starts_with(proj)` 且 scratch 更长时，若投影未做 live catch-up，
/// 表示投影滞后——调用方应先用 [`TuiTurnProjection::live_open_commentary_text`] 对齐后再判断。
fn projection_text_covers(scratch: &str, proj: &str) -> bool {
    scratch == proj || scratch.starts_with(proj) || proj.starts_with(scratch)
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
            proj.owns_streaming_content_lane(&scratch),
            "tool-phase / absorbed commentary must hide scratch duplicate"
        );
    }

    #[test]
    fn cumulative_scratch_final_not_duplicated_into_leading_commentary() {
        // 复现：scratch 跨工具轮累积「旁白+终答」；若 phase end 后仍 flush open 段，
        // 终答会被写进 before_commentary，出现在工具之前并与终答区双显。
        let mut proj = TuiTurnProjection::default();
        let mut scratch = TuiLlmStreamScratch {
            content: "先看一下 README。".into(),
            ..Default::default()
        };
        proj.apply_sse(
            &SsePayload::TurnSegmentStart {
                start: TurnSegmentStartBody {
                    segment_id: "seg-before-tc1".into(),
                    kind: "commentary".into(),
                    before_tool_call_id: Some("tc1".into()),
                },
            },
            &TuiLlmStreamScratch::default(),
        );
        scratch.content = "先看一下 README。".into();
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
        // 下一轮 LLM 继续往同一 scratch 追加终答（真实路径 push_str，不清空）
        scratch.content.push_str("总结如下。");
        let live = proj.format_projection_block(Some(&scratch));
        let commentary_at = live.find("先看一下 README。").expect("旁白");
        let tool_at = live.find("▸ read_file").expect("工具");
        let final_at = live.find("总结如下。").expect("终答");
        assert!(
            commentary_at < tool_at && tool_at < final_at,
            "须旁白→工具→终答:\n{live}"
        );
        assert_eq!(
            live.matches("总结如下。").count(),
            1,
            "终答不得双显:\n{live}"
        );
        assert!(
            !live[..tool_at].contains("总结如下。"),
            "终答不得插入工具前旁白区:\n{live}"
        );
        proj.finalize_for_display(&scratch);
        let committed = proj.format_projection_block(None);
        assert_eq!(
            committed.matches("总结如下。").count(),
            1,
            "finalize 后终答仍仅一次:\n{committed}"
        );
        let c = committed.find("先看一下 README。").expect("旁白");
        let t = committed.find("▸ read_file").expect("工具");
        let a = committed.find("总结如下。").expect("终答");
        assert!(c < t && t < a, "定稿行序:\n{committed}");
        assert!(
            !committed[..t].contains("总结如下。"),
            "定稿终答不得进旁白:\n{committed}"
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
            proj.owns_streaming_content_lane(&scratch),
            "final answer must not also stream as [assistant] tail"
        );
        proj.finalize_for_display(&scratch);
        let committed = proj.format_projection_block(None);
        assert!(
            committed.contains("总结如下。"),
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
    fn owns_lane_false_for_timeline_only_true_after_open_segment() {
        let mut proj = TuiTurnProjection::default();
        let mut scratch = TuiLlmStreamScratch {
            content: "回答".into(),
            ..Default::default()
        };
        proj.apply_sse(
            &SsePayload::TimelineLog {
                log: crate::sse::protocol::TimelineLogBody {
                    kind: "tool_result_summary".into(),
                    title: "直接执行".into(),
                    detail: None,
                },
            },
            &scratch,
        );
        assert!(
            !proj.owns_streaming_content_lane(&scratch),
            "timeline must not own content lane"
        );
        scratch.content.clear();
        proj.apply_sse(
            &SsePayload::TurnSegmentStart {
                start: TurnSegmentStartBody {
                    segment_id: "seg-1".into(),
                    kind: "commentary".into(),
                    before_tool_call_id: None,
                },
            },
            &scratch,
        );
        scratch.content.push_str("旁白");
        assert!(
            proj.owns_streaming_content_lane(&scratch),
            "open segment owns content lane"
        );
    }

    #[test]
    fn open_segment_live_catchup_shows_scratch_before_segment_end() {
        let mut proj = TuiTurnProjection::default();
        let mut scratch = TuiLlmStreamScratch {
            content: String::new(),
            ..Default::default()
        };
        proj.apply_sse(
            &SsePayload::TurnSegmentStart {
                start: TurnSegmentStartBody {
                    segment_id: "seg-1".into(),
                    kind: "commentary".into(),
                    before_tool_call_id: None,
                },
            },
            &scratch,
        );
        scratch.content.push_str("第一句。");
        let block = proj.format_projection_block(Some(&scratch));
        assert!(
            block.contains("第一句。"),
            "open segment must live-catch scratch before SegmentEnd: {block}"
        );
        assert!(
            proj.owns_streaming_content_lane(&scratch),
            "open segment lane owns content"
        );
        scratch.content.push_str("第二句。");
        let block2 = proj.format_projection_block(Some(&scratch));
        assert!(
            block2.contains("第一句。第二句。"),
            "live catch-up must grow with scratch: {block2}"
        );
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
