//! Canonical turn 归约（[`crabmate_turn_layout`]）与 `messages` 布局同步。
//!
//! 解决「旁注 delta 晚于 `tool_call` SSE」时气泡顺序错乱：段锚点 + reducer 投影后再 upsert
//! 带 `tool_call_id` 锚点的可见 assistant 行（置于对应工具之前）。

use crabmate_turn_layout::{
    PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn, TurnEvent, batch_narration_text,
    commentary_for_tool, reduce_event, streaming_commentary_block_text,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::message_dedupe::assistant_texts_fuzzy_duplicate;
use crate::sse_dispatch::TurnSegmentStartInfo;

pub(super) struct TurnCanonicalState {
    turn: Turn,
    /// 新轮次已开始（`turn_segment_start kind=answer` 清空了 `final_answer`），
    /// 但尚未收到该轮次的流式 delta。
    /// 此期间 `final_response` timeline 事件不应写入 `final_answer`，
    /// 因为后续 delta 会提供完整文本，append 会导致内容翻倍。
    awaiting_first_delta_of_new_round: bool,
}

impl TurnCanonicalState {
    pub(super) fn new() -> Self {
        Self {
            turn: Turn::default(),
            awaiting_first_delta_of_new_round: false,
        }
    }

    /// 新模型轮次开始：清空终答桶，准备接收新一轮 delta。
    pub(super) fn reset_final_answer_for_new_round(&mut self) {
        self.turn.final_answer = None;
        self.awaiting_first_delta_of_new_round = true;
    }

    pub(super) fn turn_ref(&self) -> &Turn {
        &self.turn
    }

    pub(super) fn tool_phase_open(&self) -> bool {
        self.turn.tool_phase_open
    }

    fn apply(&mut self, event: TurnEvent) {
        reduce_event(&mut self.turn, event);
    }

    fn ensure_pending_stream_segment(&mut self) {
        if self
            .turn
            .segments
            .iter()
            .any(|s| s.segment_id == PENDING_STREAM_COMMENTARY_SEGMENT_ID)
        {
            return;
        }
        self.apply(TurnEvent::SegmentStart {
            segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.to_string(),
            kind: SegmentKind::Commentary,
            before_tool_call_id: None,
        });
    }

    /// `parsing_tool_calls` demote 后：将已显示在 loading 泡内的正文迁入 canonical pending 段。
    pub(super) fn ingest_pre_tool_commentary(&mut self, text: &str) {
        if text.trim().is_empty() {
            return;
        }
        self.ensure_pending_stream_segment();
        self.apply(TurnEvent::SegmentDelta {
            segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.to_string(),
            delta: text.to_string(),
        });
    }

    pub(super) fn on_segment_start(&mut self, info: TurnSegmentStartInfo) {
        let kind = match info.kind.as_str() {
            "answer" => SegmentKind::Answer,
            _ => SegmentKind::Commentary,
        };
        self.apply(TurnEvent::SegmentStart {
            segment_id: info.segment_id,
            kind,
            before_tool_call_id: info.before_tool_call_id,
        });
    }

    pub(super) fn on_segment_end(&mut self, segment_id: String) {
        self.apply(TurnEvent::SegmentEnd { segment_id });
    }

    pub(super) fn on_tool_phase_end(&mut self) {
        self.apply(TurnEvent::ToolPhaseEnd);
    }

    /// `tool_phase_end` 已发生但仍有 open 段时的兜底（流结束投影前）。
    pub(super) fn close_open_commentary_for_projection(&mut self) {
        crabmate_turn_layout::close_open_commentary_segments(&mut self.turn);
    }

    /// 形态 B 巨泡：`final_answer` 与 batch 合并时拆回块布局。
    pub(super) fn repartition_web_block_layout_stream(&mut self) {
        crabmate_turn_layout::repartition_web_block_layout_stream(&mut self.turn);
    }

    pub(super) fn on_tool_call(&mut self, tool_call_id: &str, name: &str, summary: &str) {
        self.apply(TurnEvent::ToolCall {
            tool_call_id: tool_call_id.to_string(),
            name: name.to_string(),
            summary: summary.to_string(),
        });
    }

    /// overlay / peel 正文 append 到批说明（块布局）；不因已有 step 旁注而丢弃。
    pub(super) fn ingest_batch_commentary_from_peel(&mut self, text: &str) {
        let t = text.trim();
        if t.is_empty() {
            return;
        }
        let closed = batch_narration_text(&self.turn).unwrap_or_default();
        let open = streaming_commentary_block_text(&self.turn).unwrap_or_default();
        let mut combined = String::with_capacity(closed.len() + open.len());
        combined.push_str(&closed);
        combined.push_str(&open);
        let combined_trim = combined.trim();
        if combined_trim.ends_with(t)
            || assistant_texts_fuzzy_duplicate(combined_trim, t)
            || t == combined_trim
        {
            return;
        }
        if t.starts_with(combined_trim) && t.len() > combined_trim.len() {
            let suffix = &t[combined_trim.len()..];
            if !suffix.trim().is_empty() {
                let _ = self.try_apply_commentary_delta(suffix);
            }
            return;
        }
        let _ = self.try_apply_commentary_delta(text);
    }

    /// 将 plain `on_delta` 写入 commentary 段：优先 open 段；否则 pending / 锚点 step。
    pub(super) fn try_apply_commentary_delta(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }
        if let Some(seg_id) = self
            .turn
            .segments
            .iter()
            .rev()
            .find(|s| s.open && s.kind == SegmentKind::Commentary)
            .map(|s| s.segment_id.clone())
        {
            self.apply(TurnEvent::SegmentDelta {
                segment_id: seg_id,
                delta: delta.to_string(),
            });
            return true;
        }
        if let Some(tool_call_id) = self.turn.segments.iter().rev().find_map(|s| {
            if s.kind == SegmentKind::Commentary {
                s.before_tool_call_id.clone()
            } else {
                None
            }
        }) {
            self.apply(TurnEvent::SegmentDelta {
                segment_id: format!("seg-before-{tool_call_id}"),
                delta: delta.to_string(),
            });
            return true;
        }
        if self.turn.steps.is_empty() {
            self.ensure_pending_stream_segment();
            self.apply(TurnEvent::SegmentDelta {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.to_string(),
                delta: delta.to_string(),
            });
            return true;
        }
        let anchor_tool_call_id = self.turn.steps.iter().find_map(|s| {
            if s.before_commentary
                .as_ref()
                .is_none_or(|t| t.trim().is_empty())
                && commentary_for_tool(&self.turn, s.tool_call_id.as_str())
                    .is_none_or(|t| t.trim().is_empty())
            {
                Some(s.tool_call_id.clone())
            } else {
                None
            }
        });
        let Some(tool_call_id) = anchor_tool_call_id else {
            self.ensure_pending_stream_segment();
            self.apply(TurnEvent::SegmentDelta {
                segment_id: PENDING_STREAM_COMMENTARY_SEGMENT_ID.to_string(),
                delta: delta.to_string(),
            });
            return true;
        };
        self.apply(TurnEvent::SegmentDelta {
            segment_id: format!("seg-before-{tool_call_id}"),
            delta: delta.to_string(),
        });
        true
    }

    /// post-tool 终答 plain delta → [`TurnEvent::AnswerDelta`]（工具批结束后才生效）。
    pub(super) fn try_apply_answer_delta(&mut self, delta: &str) -> bool {
        if delta.is_empty() {
            return false;
        }
        if self.turn.tool_phase_open {
            return false;
        }
        self.awaiting_first_delta_of_new_round = false;
        self.apply(TurnEvent::AnswerDelta {
            delta: delta.to_string(),
        });
        true
    }

    /// `final_response` 时间线：写入 canonical 终答（不新增 assistant 行）；已 fuzzy 等价则 no-op。
    pub(super) fn try_ingest_final_response_text(&mut self, text: &str) -> bool {
        if text.trim().is_empty() || self.turn.tool_phase_open {
            return false;
        }
        // 新轮次已开始但尚未收到流式 delta：
        // `final_response` 是上一轮 LLM 调用的终答总结，
        // 不应写入已清空的 `final_answer`——后续 delta 会提供完整文本，
        // append 会导致 "final_response文本 + delta文本" 翻倍。
        if self.awaiting_first_delta_of_new_round {
            // 如果流式 delta 已被路由到 commentary（而非 final_answer），
            // final_answer 仍为空，此时 final_response 应写入以补全终答。
            self.awaiting_first_delta_of_new_round = false;
            if self.turn.final_answer.is_none() {
                self.apply(TurnEvent::AnswerDelta {
                    delta: text.to_string(),
                });
            }
            return true;
        }
        if self.turn.final_answer.is_some() {
            // 已有流式正文，不再用 final_response 替换——保留流式阶段的真实文本
            return true;
        }
        self.apply(TurnEvent::AnswerDelta {
            delta: text.to_string(),
        });
        true
    }

    /// 块布局批说明字符数（形态 B 短终答门控）。
    pub(super) fn batch_narration_char_len(&self) -> usize {
        crabmate_turn_layout::batch_narration_text(&self.turn)
            .map(|t| t.chars().count())
            .unwrap_or(0)
    }

    /// 块布局批说明全文（[`crabmate_turn_layout::batch_narration_text`]）— 测试用；生产路径见 `project_turn_web`。
    #[cfg(test)]
    pub(super) fn batch_narration_text(&self) -> Option<String> {
        crabmate_turn_layout::batch_narration_text(&self.turn)
    }

    /// 首个 `tool_call` 前：把尾泡 / 误写入的 `final_answer` 收进 pending 旁注段并清空终答桶。
    pub(super) fn absorb_pre_tool_narration_for_first_tool(&mut self, from_bubble: &str) {
        if !from_bubble.trim().is_empty() {
            self.ingest_pre_tool_commentary(from_bubble);
        }
        if let Some(fa) = self.turn.final_answer.take() {
            if !fa.trim().is_empty()
                && (from_bubble.trim().is_empty()
                    || !assistant_texts_fuzzy_duplicate(from_bubble, fa.as_str()))
            {
                self.ingest_pre_tool_commentary(fa.as_str());
            }
        }
    }

    /// 读取某 `tool_call_id` 对应工具前旁注（reducer 步 + 未 flush 段）；单测与排障用。
    #[cfg(test)]
    pub(super) fn commentary_before_tool(&self, tool_call_id: &str) -> Option<String> {
        let mut text = self
            .turn
            .steps
            .iter()
            .find(|s| s.tool_call_id == tool_call_id)
            .and_then(|s| s.before_commentary.clone())
            .unwrap_or_default();
        for seg in &self.turn.segments {
            if seg.kind == SegmentKind::Commentary
                && seg.before_tool_call_id.as_deref() == Some(tool_call_id)
                && !seg.text.is_empty()
            {
                text.push_str(&seg.text);
            }
        }
        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
    }
}

pub(super) fn make_turn_canonical_cell() -> Rc<RefCell<TurnCanonicalState>> {
    Rc::new(RefCell::new(TurnCanonicalState::new()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message_dedupe::normalize_assistant_text_for_dedupe;
    use crate::sse_dispatch::TurnSegmentStartInfo;

    #[test]
    fn late_delta_attaches_to_first_tool_missing_commentary() {
        let mut turn = TurnCanonicalState::new();
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_create".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_create".into()),
        });
        turn.on_tool_call("tc_read", "read_dir", "read dir");
        turn.on_tool_call("tc_create", "create_file", "create file");
        assert!(turn.try_apply_commentary_delta("工作区是空的。"));
        assert_eq!(
            turn.commentary_before_tool("tc_create").as_deref(),
            Some("工作区是空的。")
        );
        assert!(turn.commentary_before_tool("tc_read").is_none());
    }

    #[test]
    fn pre_tool_delta_buffers_in_pending_segment() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_commentary_delta("好的，先解压。"));
        turn.on_tool_call("tc_unpack", "unpack", "unpack");
        assert_eq!(
            turn.commentary_before_tool("tc_unpack").as_deref(),
            Some("好的，先解压。")
        );
    }

    #[test]
    fn absorb_pre_tool_clears_migrated_final_answer() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_answer_delta("误写入终答桶。"));
        turn.absorb_pre_tool_narration_for_first_tool("尾泡旁注。");
        assert!(turn.turn_ref().final_answer.is_none());
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        assert_eq!(
            turn.commentary_before_tool("tc_a").as_deref(),
            Some("尾泡旁注。误写入终答桶。")
        );
    }

    #[test]
    fn ingest_batch_commentary_appends_after_first_tool_step() {
        let mut turn = TurnCanonicalState::new();
        turn.ingest_pre_tool_commentary("先解压。");
        turn.on_tool_call("tc1", "unpack", "unpack");
        turn.ingest_batch_commentary_from_peel("再看 INSTALL。");
        turn.on_tool_call("tc2", "read_file", "read file");
        assert_eq!(
            turn.batch_narration_text().as_deref(),
            Some("先解压。再看 INSTALL。")
        );
    }

    #[test]
    fn batch_narration_text_merges_pending_and_step_commentary() {
        let mut turn = TurnCanonicalState::new();
        turn.ingest_batch_commentary_from_peel("pending。");
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_a".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_a".into()),
        });
        assert!(turn.try_apply_commentary_delta("步骤 A。"));
        turn.on_segment_end("seg-before-tc_a".into());
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_b".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_b".into()),
        });
        assert!(turn.try_apply_commentary_delta("步骤 B。"));
        turn.on_segment_end("seg-before-tc_b".into());
        turn.on_tool_call("tc_b", "tool_b", "tool b");
        assert_eq!(
            turn.batch_narration_text().as_deref(),
            Some("pending。步骤 A。步骤 B。")
        );
    }

    #[test]
    fn ingest_pre_tool_commentary_migrates_demoted_bubble() {
        let mut turn = TurnCanonicalState::new();
        turn.ingest_pre_tool_commentary("整段 narration。");
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        assert_eq!(
            turn.commentary_before_tool("tc_a").as_deref(),
            Some("整段 narration。")
        );
    }

    #[test]
    fn double_ingest_pre_tool_duplicates_commentary_text() {
        let mut turn = TurnCanonicalState::new();
        turn.ingest_pre_tool_commentary("好的，先解压。");
        turn.ingest_pre_tool_commentary("好的，先解压。");
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        assert_eq!(
            turn.commentary_before_tool("tc_a").as_deref(),
            Some("好的，先解压。好的，先解压。")
        );
    }

    #[test]
    fn second_tool_commentary_must_not_receive_prior_final_answer() {
        let mut turn = TurnCanonicalState::new();
        turn.ingest_pre_tool_commentary("先解压。");
        turn.on_tool_call("tc1", "tool_a", "tool a");
        turn.on_tool_phase_end();
        turn.try_apply_answer_delta("段一。");
        turn.try_apply_answer_delta("段二。");
        // post-tool 终答不得再 `ingest_pre_tool_commentary`（见 `demote_answer_before_tools` 门控）。
        turn.on_tool_call("tc2", "tool_b", "tool b");
        assert!(turn.commentary_before_tool("tc2").is_none());
        assert_eq!(
            turn.turn_ref().final_answer.as_deref(),
            Some("段一。段二。")
        );
    }

    #[test]
    fn answer_delta_blocked_while_tool_phase_open() {
        let mut turn = TurnCanonicalState::new();
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        assert!(!turn.try_apply_answer_delta("不应写入。"));
        turn.on_segment_start(TurnSegmentStartInfo {
            segment_id: "seg-before-tc_b".into(),
            kind: "commentary".into(),
            before_tool_call_id: Some("tc_b".into()),
        });
        assert!(turn.try_apply_commentary_delta("工具前旁注。"));
        assert_eq!(
            crabmate_turn_layout::streaming_commentary_block_text(turn.turn_ref()).as_deref(),
            Some("工具前旁注。")
        );
        turn.on_tool_phase_end();
        assert!(turn.try_apply_answer_delta("完成。"));
        assert_eq!(
            normalize_assistant_text_for_dedupe(turn.turn_ref().final_answer.as_deref().unwrap()),
            normalize_assistant_text_for_dedupe("完成。")
        );
    }

    #[test]
    fn ingest_final_response_extends_shorter_stream_with_timeline_detail() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_answer_delta("当前目录下有三个压缩包。"));
        assert!(turn.try_ingest_final_response_text("当前目录下有三个压缩包：\n\n1. **A** — x"));
        // final_response 不再替换已有流式正文
        assert_eq!(
            turn.turn_ref().final_answer.as_deref(),
            Some("当前目录下有三个压缩包。")
        );
    }

    #[test]
    fn ingest_final_response_keeps_stream_text_when_already_streamed() {
        let mut turn = TurnCanonicalState::new();
        assert!(turn.try_apply_answer_delta("短。"));
        assert!(turn.try_ingest_final_response_text("短。完整终答段落。"));
        // final_response 不再替换已有流式正文
        assert_eq!(turn.turn_ref().final_answer.as_deref(), Some("短。"));
    }

    #[test]
    fn final_response_after_rotation_writes_when_no_delta_arrived() {
        let mut turn = TurnCanonicalState::new();
        // 旧轮次：流式 delta 写入
        assert!(turn.try_apply_answer_delta("旧轮正文。"));
        // 新轮次开始：reset 清空 final_answer 并设 awaiting 标志
        turn.reset_final_answer_for_new_round();
        assert!(turn.turn_ref().final_answer.is_none());
        // final_response 在 reset 之后、新 delta 之前到达
        // 流式 delta 被路由到 commentary 时，final_answer 仍为空，
        // final_response 应写入以补全终答。
        assert!(turn.try_ingest_final_response_text("旧轮正文。"));
        assert_eq!(turn.turn_ref().final_answer.as_deref(), Some("旧轮正文。"));
        // 新轮次 delta 到达 → append 到已有 final_answer
        assert!(turn.try_apply_answer_delta("新轮正文。"));
        assert_eq!(
            turn.turn_ref().final_answer.as_deref(),
            Some("旧轮正文。新轮正文。")
        );
    }

    #[test]
    fn final_response_after_rotation_does_not_replace_existing_answer() {
        let mut turn = TurnCanonicalState::new();
        // 新轮次开始：reset
        turn.reset_final_answer_for_new_round();
        // 新轮次 delta 先到达 → 写入 final_answer，清除 awaiting
        assert!(turn.try_apply_answer_delta("流式正文。"));
        assert_eq!(turn.turn_ref().final_answer.as_deref(), Some("流式正文。"));
        // final_response 后到达 → 已有流式正文，不替换
        assert!(turn.try_ingest_final_response_text("旧轮总结。"));
        assert_eq!(turn.turn_ref().final_answer.as_deref(), Some("流式正文。"));
    }

    #[test]
    fn final_response_writes_when_deltas_routed_to_commentary() {
        let mut turn = TurnCanonicalState::new();
        // 模拟 llm-24 场景：新轮次开始
        turn.reset_final_answer_for_new_round();
        // 流式 delta 被路由到 commentary（非 final_answer），
        // 模拟 `try_apply_commentary_delta` 调用
        let _ =
            turn.try_apply_commentary_delta("已创建 `README.md`，包含构建步骤、选项说明和示例。");
        assert!(turn.turn_ref().final_answer.is_none());
        // final_response 到达 → 应写入 final_answer（因为 deltas 去了 commentary）
        assert!(
            turn.try_ingest_final_response_text(
                "已创建 `README.md`，包含构建步骤、选项说明和示例。"
            )
        );
        assert_eq!(
            turn.turn_ref().final_answer.as_deref(),
            Some("已创建 `README.md`，包含构建步骤、选项说明和示例。")
        );
    }

    #[test]
    fn final_response_before_rotation_is_written_normally() {
        let mut turn = TurnCanonicalState::new();
        // 首轮（无 rotation）：final_response 到达时 final_answer 为空
        assert!(turn.try_ingest_final_response_text("终答文本。"));
        assert_eq!(turn.turn_ref().final_answer.as_deref(), Some("终答文本。"));
    }
}
