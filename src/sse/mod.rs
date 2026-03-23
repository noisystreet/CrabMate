//! SSE：经 `data:` 下发的**控制面 JSON**（`protocol`）与客户端侧**行分类**（`line`）。
//!
//! 与 `llm::api::stream_chat` 下发的纯文本 delta 区分；前端对齐见 `frontend/src/api.ts`。

pub mod line;
pub mod protocol;

pub use line::{AgentLineKind, classify_agent_sse_line};
pub use protocol::{
    CommandApprovalBody, SseErrorBody, SsePayload, StagedPlanFinishedBody, StagedPlanStartedBody,
    StagedPlanStepFinishedBody, StagedPlanStepStartedBody, ToolResultBody, encode_message,
};
