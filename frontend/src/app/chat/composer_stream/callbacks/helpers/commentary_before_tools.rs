//! 同一模型轮次含 `tool_calls` 时，将已流出的正文降级为旁注（OpenClaw/Ethos 式 commentary 抑制）。

use crate::stream_text_overlay::{
    demote_assistant_message_answer_to_commentary, stream_overlay_demote_answer_to_reasoning,
    stream_overlay_take_into_stored_message,
};

use super::super::super::context::ChatStreamCallbackCtx;
use super::super::super::per_stream_accum::PerStreamAccum;

/// `parsing_tool_calls: true` 或 `on_tool_call`：本轮若已有正文 delta，不再当终答展示。
pub(crate) fn suppress_assistant_answer_as_commentary_before_tools(
    stream_ctx: &ChatStreamCallbackCtx,
    accum: &PerStreamAccum,
) {
    stream_ctx.scratch.enter_commentary_before_tools_lane();
    let sid = stream_ctx.bound_stream_session_id.clone();
    let mid = stream_ctx.scratch.clone_assistant_id();
    stream_overlay_demote_answer_to_reasoning(
        stream_ctx.chat.stream_text_overlay,
        sid.as_str(),
        mid.as_str(),
    );
    stream_ctx.update_bound_session(|session| {
        let Some(idx) = session.messages.iter().position(|m| m.id == mid) else {
            return;
        };
        stream_overlay_take_into_stored_message(
            stream_ctx.chat.stream_text_overlay,
            sid.as_str(),
            mid.as_str(),
            &mut session.messages[idx],
        );
        demote_assistant_message_answer_to_commentary(&mut session.messages[idx]);
    });
    accum.clear_answer_delta_chars();
}
