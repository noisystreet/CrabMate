use serde::{Deserialize, Serialize};

/// LLM 流式阶段、首个 `turn_segment_start` 到达前的 plain delta 缓冲段（无 `before_tool_call_id`）。
pub const PENDING_STREAM_COMMENTARY_SEGMENT_ID: &str = "pending-stream-commentary";

/// 段类型：`commentary` 在工具前旁注；`answer` 为终答（整轮工具结束后）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentKind {
    Commentary,
    Answer,
}

/// 进行中的段（`turn_segment_start` … delta … `turn_segment_end`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnSegment {
    pub segment_id: String,
    pub kind: SegmentKind,
    /// 若非空：本段正文展示在该 `tool_call_id` **之前**（晚到 delta 仍挂在此锚点）。
    pub before_tool_call_id: Option<String>,
    pub text: String,
    pub open: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolStep {
    pub tool_call_id: String,
    pub name: String,
    pub summary: String,
    pub before_commentary: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub pre_tool_timeline: Vec<String>,
    pub steps: Vec<ToolStep>,
    pub segments: Vec<TurnSegment>,
    pub tool_phase_open: bool,
}

impl Turn {
    pub fn segment_by_id_mut(&mut self, id: &str) -> Option<&mut TurnSegment> {
        self.segments.iter_mut().find(|s| s.segment_id == id)
    }

    pub fn step_by_call_id_mut(&mut self, id: &str) -> Option<&mut ToolStep> {
        self.steps.iter_mut().find(|s| s.tool_call_id == id)
    }

    pub fn step_by_call_id(&self, id: &str) -> Option<&ToolStep> {
        self.steps.iter().find(|s| s.tool_call_id == id)
    }
}
