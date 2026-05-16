//! SSE 回调闭包工厂：拆分为子模块以降低单文件体量（装配仍见 [`super::assemble`]）。

mod stream_end;
mod timeline_dispatch;
mod tool_callbacks;

pub(super) use stream_end::{
    chat_stream_on_done_builder, chat_stream_on_error_builder, chat_stream_on_ws_builder,
};
pub(super) use timeline_dispatch::make_on_timeline_log;
pub(super) use tool_callbacks::{
    chat_stream_on_tool_call_builder, make_on_tool_output_chunk, make_on_tool_result,
};
