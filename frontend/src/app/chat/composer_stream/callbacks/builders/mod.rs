//! SSE 回调闭包工厂：拆分为子模块以降低单文件体量（装配仍见 [`super::assemble`]）。

mod stream_end;
mod stream_phase_hooks;
mod timeline_dispatch;
mod tool_callbacks;
mod turn_layout_callbacks;

pub(super) use stream_end::{
    chat_stream_on_done_builder, chat_stream_on_error_builder, chat_stream_on_ws_builder,
};
pub(super) use stream_phase_hooks::{
    make_on_assistant_answer_phase_with_stream_phase, make_on_stream_ended_with_stream_phase,
    make_on_tool_status_with_stream_phase,
};
pub(super) use timeline_dispatch::make_on_timeline_log;
pub(super) use tool_callbacks::{
    chat_stream_on_tool_call_builder, make_on_tool_output_chunk, make_on_tool_result,
};
pub(super) use turn_layout_callbacks::{
    make_on_turn_segment_end, make_on_turn_segment_start, make_on_turn_tool_phase_end,
};
