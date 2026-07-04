//! Canonical turn 归约（[`crabmate_turn_layout`]）与 `messages` 布局同步。
//!
//! 解决「旁注 delta 晚于 `tool_call` SSE」时气泡顺序错乱：段锚点 + reducer 投影后再 upsert
//! 带 `tool_call_id` 锚点的可见 assistant 行（置于对应工具之前）。

use crabmate_turn_layout::{
    PENDING_STREAM_COMMENTARY_SEGMENT_ID, SegmentKind, Turn, TurnEvent, reduce_event,
};
use std::cell::RefCell;
use std::rc::Rc;

use crate::sse_dispatch::TurnSegmentStartInfo;

pub(super) struct TurnCanonicalState {
    turn: Turn,
}

impl TurnCanonicalState {
    pub(super) fn new() -> Self {
        Self {
            turn: Turn::default(),
        }
    }

    pub(super) fn turn_ref(&self) -> &Turn {
        &self.turn
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

    pub(super) fn on_tool_call(&mut self, tool_call_id: &str, name: &str, summary: &str) {
        self.apply(TurnEvent::ToolCall {
            tool_call_id: tool_call_id.to_string(),
            name: name.to_string(),
            summary: summary.to_string(),
        });
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
    fn ingest_pre_tool_commentary_migrates_demoted_bubble() {
        let mut turn = TurnCanonicalState::new();
        turn.ingest_pre_tool_commentary("整段 narration。");
        turn.on_tool_call("tc_a", "tool_a", "tool a");
        assert_eq!(
            turn.commentary_before_tool("tc_a").as_deref(),
            Some("整段 narration。")
        );
    }
}
