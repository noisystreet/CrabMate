//! CrabMate **`POST /chat/stream`** 控制面 JSON 的**协议版本**常量与 **`stop`/`handled`/`plain`** 分类。
//!
//! - **`SSE_PROTOCOL_VERSION`**：与 `docs/SSE协议.md` 中的 **`v`** / `sse_capabilities.supported_sse_v` 一致。
//! - **[`classify_sse_control_outcome`]**：与 Leptos **`frontend-leptos/src/sse_dispatch.rs`** 同序；金样 **`fixtures/sse_control_golden.jsonl`**。

mod control_classify;
mod control_extract;
mod sse_frame;
mod stream_end_reason;

pub use control_classify::{classify_sse_control_outcome, key_present_non_null};
pub use control_extract::{
    SseClarificationField, SseClarificationQuestionnaire, SseErrorStop, SseStagedPlanStepEnd,
    SseStagedPlanStepStart, SseThinkingTrace, SseTimelineLog, SseToolCall, SseToolResult,
    extract_clarification_questionnaire, extract_error_stop, extract_staged_plan_step_finished,
    extract_staged_plan_step_started, extract_thinking_trace, extract_timeline_log,
    extract_tool_call, extract_tool_result,
};
pub use sse_frame::{
    extract_stream_ended_reason, is_sse_done_sentinel, join_sse_data_lines, parse_sse_event_id,
};
pub use stream_end_reason::StreamEndReason;

/// 当前控制面版本：信封顶层 **`v`**，以及首帧 **`sse_capabilities.supported_sse_v`**。
pub const SSE_PROTOCOL_VERSION: u8 = 1;

#[cfg(test)]
mod tests {
    use super::SSE_PROTOCOL_VERSION;
    use std::path::PathBuf;

    /// 文档中的「当前版本」须与本常量一致（bump 版本时同步改 `docs/SSE协议.md` / `docs/en/SSE_PROTOCOL.md`）。
    #[test]
    fn sse_protocol_md_lists_current_version() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let zh = root.join("../../docs/SSE协议.md");
        let en = root.join("../../docs/en/SSE_PROTOCOL.md");
        let zh_s =
            std::fs::read_to_string(&zh).unwrap_or_else(|e| panic!("read {}: {e}", zh.display()));
        let en_s =
            std::fs::read_to_string(&en).unwrap_or_else(|e| panic!("read {}: {e}", en.display()));
        let needle = format!("**`{SSE_PROTOCOL_VERSION}`**");
        assert!(
            zh_s.contains(&needle),
            "{} must contain current version marker {needle}",
            zh.display()
        );
        assert!(
            en_s.contains(&needle),
            "{} must contain current version marker {needle}",
            en.display()
        );
    }
}
