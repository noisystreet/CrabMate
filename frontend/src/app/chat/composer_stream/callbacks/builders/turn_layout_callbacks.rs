//! `turn_segment_*` / `turn_tool_phase_end` SSE 回调工厂。

use std::rc::Rc;

use crate::sse_dispatch::TurnSegmentStartInfo;

use super::super::turn_layout::TurnLayout;

use super::super::super::context::ChatStreamCallbackCtx;

pub(in super::super) fn make_on_turn_segment_start(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(TurnSegmentStartInfo)> {
    Rc::new(move |info: TurnSegmentStartInfo| {
        if stream_ctx.is_stale() {
            return;
        }
        // kind == "answer" 表示新一轮 LLM 调用开始（outer loop 非首轮），
        // 此时应结束当前 loading 气泡并创建新气泡。
        if info.kind == "answer" {
            TurnLayout::rotate_followup_model_round(stream_ctx.as_ref());
            stream_ctx
                .scratch
                .reset_canonical_final_answer_for_new_round();
        }
        stream_ctx.scratch.on_turn_segment_start(info);
        stream_ctx.scratch.sync_turn_projection(stream_ctx.as_ref());
        TurnLayout::reset_loading_tail_streaming_text(stream_ctx.as_ref());
        stream_ctx.scratch.sync_stream_preview(stream_ctx.as_ref());
    })
}

pub(in super::super) fn make_on_turn_segment_end(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn(String)> {
    Rc::new(move |segment_id: String| {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx.scratch.on_turn_segment_end(segment_id);
        stream_ctx.scratch.sync_turn_projection(stream_ctx.as_ref());
        stream_ctx.scratch.sync_stream_preview(stream_ctx.as_ref());
    })
}

pub(in super::super) fn make_on_turn_tool_phase_end(
    stream_ctx: Rc<ChatStreamCallbackCtx>,
) -> Rc<dyn Fn()> {
    Rc::new(move || {
        if stream_ctx.is_stale() {
            return;
        }
        stream_ctx.scratch.on_turn_tool_phase_end();
        stream_ctx.scratch.sync_turn_projection(stream_ctx.as_ref());
        stream_ctx.scratch.sync_stream_preview(stream_ctx.as_ref());
    })
}
