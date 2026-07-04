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
