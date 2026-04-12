//! SSE：经 `data:` 下发的**控制面 JSON**（`protocol`）与客户端侧**行分类**（`line`）。
//!
//! 与 `llm::api::stream_chat` 下发的纯文本 delta 区分；前端对齐见 `frontend-leptos/src/api.rs`。
//!
//! 人读契约见仓库 **`docs/SSE_PROTOCOL.md`**。协议版本常量见 **`protocol::SSE_PROTOCOL_VERSION`**（workspace crate **`crabmate-sse-protocol`**，与 Leptos **`frontend-leptos/src/api.rs`** 同源）。
//!
//! 控制面 **`stop`/`handled`/`plain`** 分类见 workspace crate **`crabmate-sse-protocol`**（`classify_sse_control_outcome`），金样 **`fixtures/sse_control_golden.jsonl`**。

pub mod line;
mod mpsc_send;
pub mod protocol;
pub(crate) mod stream_hub;
pub(crate) mod web_approval;

pub(crate) use mpsc_send::{send_string_logged, send_string_logged_cooperative_cancel};
pub(crate) use stream_hub::SseStreamHub;

pub use protocol::{
    ClarificationQuestionField, ClarificationQuestionnaireBody, CommandApprovalBody,
    ConversationSavedBody, SseCapabilitiesBody, SseErrorBody, SsePayload, StagedPlanFinishedBody,
    StagedPlanStartedBody, StagedPlanStepFinishedBody, StagedPlanStepStartedBody, StreamEndedBody,
    TimelineLogBody, ToolCallSummary, ToolResultBody, encode_message,
};
