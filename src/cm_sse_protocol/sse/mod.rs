//! SSE：经 `data:` 下发的**控制面 JSON**（`protocol`）与客户端侧**行分类**（`line`）。
//!
//! 与 `llm::api::stream_chat` 下发的纯文本 delta 区分；前端对齐见 Client **`frontend/src/api/`**（**`chat_stream/`**）。
//!
//! 人读契约见仓库 **`docs/SSE协议.md`**。协议版本常量见 **`crabmate-sse-protocol`** 的 `SSE_PROTOCOL_VERSION`。

mod ag_ui_convert;
mod ag_ui_encode;
mod ag_ui_event;
#[cfg(feature = "server")]
mod control_mirror;
mod encoder;
mod encoder_v2;
#[cfg(feature = "server")]
mod final_response_terminal;
pub mod line;
#[cfg(feature = "server")]
mod mpsc_send;
pub mod protocol;
#[cfg(feature = "server")]
pub mod stream_hub;
#[cfg(feature = "server")]
pub mod web_approval;

#[cfg(feature = "server")]
pub use control_mirror::send_sse_control_payload_optional;
pub use encoder::{SseEncoder, default_encoder};
pub use encoder_v2::V2Encoder;
#[cfg(feature = "server")]
pub use final_response_terminal::{
    encode_reasoning_message_content_sse, encode_text_message_content_sse,
    encode_text_message_start_sse_str, send_final_response_timeline_then_answer_phase,
    send_reasoning_message_content_sse, send_reasoning_message_end_sse,
    send_reasoning_message_start_sse, send_run_started_sse, send_state_snapshot_sse,
    send_text_message_end_sse, send_text_message_start_sse,
};
#[cfg(feature = "server")]
pub use mpsc_send::{send_string_logged, send_string_logged_cooperative_cancel};
#[cfg(feature = "server")]
pub use stream_hub::SseStreamHub;

#[cfg(feature = "server")]
pub use control_mirror::SseControlMirror;
pub use protocol::{
    ClarificationQuestionField, ClarificationQuestionnaireBody, CommandApprovalBody,
    ConversationSavedBody, SseCapabilitiesBody, SseErrorBody, SsePayload, StreamDrainingBody,
    StreamEndedBody, ThinkingTraceBody, TimelineLogBody, ToolCallSummary, ToolOutputChunkBody,
    ToolResultBody, TurnSegmentEndBody, TurnSegmentStartBody, encode_message,
};
