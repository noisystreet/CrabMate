//! 单次 `/chat/stream` attach 内与 **SSE 回调树** 共享的可变草稿：`PerStreamAccum` + 输出车道 `Cell`。
//!
//! 与 [`super::context::ChatStreamCallbackCtx`] 分工：ctx 绑定会话、tail、shell；本结构只承载「车道 + 单轮累计」，
//! 由 [`super::callbacks::assemble::build_chat_stream_callbacks`] 一次性创建并传入各 `on_*` 工厂。
//! 回合收尾请经 [`super::per_stream_accum::PerStreamAccum::summarize_for_stream_done`] 读取累计，避免闭包漏字段。

use std::rc::Rc;

use super::callbacks::stream_turn_state::{StreamOutputLaneCell, new_stream_output_lane_cell};
use super::per_stream_accum::PerStreamAccum;

/// 单轮流 SSE 回调共享的 lane + 累计（`Clone` 仅为传递 `Rc` 句柄）。
#[derive(Clone)]
pub(super) struct StreamSseScratch {
    pub(super) lane: StreamOutputLaneCell,
    pub(super) accum: Rc<PerStreamAccum>,
}

impl StreamSseScratch {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            lane: new_stream_output_lane_cell(),
            accum: PerStreamAccum::new_rc(),
        }
    }
}
