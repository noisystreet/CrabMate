//! SSE：经 `data:` 下发的**控制面 JSON**（`protocol`）与客户端侧**行分类**（`line`）。
//!
//! 与 `llm::api::stream_chat` 下发的纯文本 delta 区分；前端对齐见 `frontend/src/api.ts`。
//!
//! 人读契约见仓库 **`docs/SSE_PROTOCOL.md`**。协议版本常量见 **`protocol::SSE_PROTOCOL_VERSION`**（与 `docs/SSE_PROTOCOL.md` 及前端 `api.ts` 的 `SSE_PROTOCOL_VERSION` 对齐）。

pub mod line;
mod mpsc_send;
pub mod protocol;

pub(crate) use mpsc_send::send_string_logged;

pub use protocol::{
    CommandApprovalBody, SseErrorBody, SsePayload, StagedPlanFinishedBody, StagedPlanStartedBody,
    StagedPlanStepFinishedBody, StagedPlanStepStartedBody, ToolResultBody, encode_message,
};
