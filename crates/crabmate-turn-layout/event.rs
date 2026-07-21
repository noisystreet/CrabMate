use serde::{Deserialize, Serialize};

use crate::model::SegmentKind;

/// Reducer 输入：与 SSE 控制面 / 内部编排对齐（JSON 金样可直接反序列化）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TurnEvent {
    TimelineAssistant {
        text: String,
    },
    SegmentStart {
        segment_id: String,
        kind: SegmentKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        before_tool_call_id: Option<String>,
    },
    SegmentDelta {
        segment_id: String,
        delta: String,
    },
    SegmentEnd {
        segment_id: String,
    },
    ToolCall {
        tool_call_id: String,
        name: String,
        summary: String,
    },
    ToolPhaseEnd,
}
