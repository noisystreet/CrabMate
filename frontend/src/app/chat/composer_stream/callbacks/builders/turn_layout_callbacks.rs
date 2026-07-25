//! `turn_segment_*` / `turn_tool_phase_end` SSE 回调工厂。

use std::rc::Rc;

use crate::sse_dispatch::TurnSegmentStartInfo;

use super::super::turn_layout::{BubbleRotationSemantics, TurnLayout};

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
        //
        // 注意：此处用 Cleanup 语义（仅轮换，lane 保持 Reasoning），因为后端保证
        // turn_segment_start(kind="answer") 之后必然跟随 assistant_answer_phase 事件，
        // 由后者将 lane 推进到 Answering。若用 ContinueAnswering 会导致双重轮换：
        //   1. turn_segment_start 中 ContinueAnswering 设置 lane = Answering
        //   2. 随后 assistant_answer_phase 事件将 lane 推进到 AnsweringPendingFollowupBubble
        //   3. 导致下一个 on_delta 触发第二次不必要的轮换
        if info.kind == "answer" {
            TurnLayout::rotate_bubble(stream_ctx.as_ref(), BubbleRotationSemantics::Cleanup);
            stream_ctx.scratch.reset_answer_state_for_new_round();
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
