//! SSE：经 `data:` 下发的**控制面 JSON**（`protocol`）与客户端侧**行分类**（`line`）。
//!
//! 与 `llm::api::stream_chat` 下发的纯文本 delta 区分；前端对齐见 **`frontend/src/api/`**（**`chat_stream/`**）。
//!
//! 人读契约见仓库 **`docs/SSE协议.md`**。协议版本常量见 **`protocol::SSE_PROTOCOL_VERSION`**（workspace crate **`crabmate-sse-protocol`**，与 Leptos **`frontend/src/api/mod.rs`** 同源）。
//!
//! 控制面 **`stop`/`handled`/`plain`** 分类见 workspace crate **`crabmate-sse-protocol`**（`classify_sse_control_outcome`），金样 **`fixtures/sse_control_golden.jsonl`**。

mod ag_ui_convert;
mod ag_ui_encode;
mod ag_ui_event;
mod control_mirror;
mod encoder;
mod encoder_v2;
mod final_response_terminal;
pub mod line;
mod mpsc_send;
pub mod protocol;
pub mod stream_hub;
pub mod web_approval;

pub use control_mirror::send_sse_control_payload_optional;
pub use encoder::{SseEncoder, V1Encoder, default_encoder, resolve_encoder};
pub use encoder_v2::V2Encoder;
pub use final_response_terminal::{
    encode_reasoning_message_content_sse, encode_text_message_content_sse,
    send_final_response_timeline_then_answer_phase, send_reasoning_message_content_sse,
    send_reasoning_message_end_sse, send_reasoning_message_start_sse, send_run_started_sse,
    send_state_snapshot_sse, send_text_message_end_sse, send_text_message_start_sse,
};
pub use mpsc_send::{send_string_logged, send_string_logged_cooperative_cancel};
pub use stream_hub::SseStreamHub;

pub use control_mirror::SseControlMirror;
pub use protocol::{
    ClarificationQuestionField, ClarificationQuestionnaireBody, CommandApprovalBody,
    ConversationSavedBody, SseCapabilitiesBody, SseErrorBody, SsePayload, StagedPlanFinishedBody,
    StagedPlanStartedBody, StagedPlanStepFinishedBody, StagedPlanStepStartedBody, StreamEndedBody,
    ThinkingTraceBody, TimelineLogBody, ToolCallSummary, ToolOutputChunkBody, ToolResultBody,
    TurnSegmentEndBody, TurnSegmentStartBody, encode_message,
};
