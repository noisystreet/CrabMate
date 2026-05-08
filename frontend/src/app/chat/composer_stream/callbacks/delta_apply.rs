//! SSE `on_delta` 正文/思维链写入与车道轮换（从 [`super::builders::chat_stream_on_delta_builder`] 拆出以便单测与降 nloc）。

use std::rc::Rc;

use super::super::context::ChatStreamCallbackCtx;
use super::super::per_stream_accum::PerStreamAccum;
use super::helpers::rotate_streaming_assistant_for_followup_model_round;
use super::stream_session_access::append_stream_assistant_chunk;
use super::stream_turn_state::{StreamOutputLaneCell, lane_take_followup_rotation_pending};

/// 将单块流式文本写入当前尾泡：必要时先轮换占位，再按车道写入正文或 `reasoning_text`。
pub(super) fn apply_chat_stream_text_delta(
    stream_ctx: &ChatStreamCallbackCtx,
    output_lane: &StreamOutputLaneCell,
    accum: &PerStreamAccum,
    chunk: &str,
) {
    if lane_take_followup_rotation_pending(output_lane.as_ref()) {
        rotate_streaming_assistant_for_followup_model_round(stream_ctx);
        accum.clear_answer_delta_chars();
    }
    let mid = stream_ctx.tail.borrow_assistant_id();
    let lane = output_lane.get();
    if lane.in_answer_body_lane() {
        accum.add_answer_delta_chars(chunk.chars().count());
        append_stream_assistant_chunk(stream_ctx, mid.as_str(), chunk, false);
    } else {
        append_stream_assistant_chunk(stream_ctx, mid.as_str(), chunk, true);
    }
}

/// 装配 `on_delta` 闭包（与 [`assemble::build_chat_stream_callbacks`] 对齐）。
pub(super) fn chat_stream_on_delta_builder(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
    output_lane: StreamOutputLaneCell,
    accum: Rc<PerStreamAccum>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |chunk: String| {
        apply_chat_stream_text_delta(stream_ctx.as_ref(), &output_lane, accum.as_ref(), &chunk);
    })
}
