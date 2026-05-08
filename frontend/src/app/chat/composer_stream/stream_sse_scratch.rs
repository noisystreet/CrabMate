//! 单次 `/chat/stream` attach 内 SSE 回调共享的 **Cell/RefCell 收口**：
//! 输出车道、单轮累计、尾气泡 id 与工具 FIFO 同处 [`StreamSseScratchInner`]，经 [`StreamSseScratch`] 的 `Rc` 共享，
//! 避免在 `ChatStreamCallbackCtx` 与独立 `scratch` 参数间分裂可变状态。
//!
//! 与 [`super::context::ChatStreamCallbackCtx`] 分工：ctx 绑定会话、shell、本 scratch；回合收尾仍经
//! [`super::per_stream_accum::PerStreamAccum::summarize_for_stream_done`]。

use std::cell::{Cell, Ref, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use super::per_stream_accum::PerStreamAccum;
use super::stream_turn_state::{StreamOutputLaneCell, new_stream_output_lane_cell};

struct StreamSseScratchInner {
    lane: StreamOutputLaneCell,
    accum: Rc<PerStreamAccum>,
    assistant_message_id: RefCell<String>,
    post_tool_stream_tail: Cell<bool>,
    pending_tool_message_ids: Rc<RefCell<VecDeque<String>>>,
}

/// 单轮流 SSE 回调共享的 lane + 累计 + 尾泡可变状态（`Clone` 仅为传递 `Rc` 句柄）。
#[derive(Clone)]
pub(super) struct StreamSseScratch {
    inner: Rc<StreamSseScratchInner>,
}

impl StreamSseScratch {
    #[must_use]
    pub(super) fn new(initial_asst_id: String) -> Self {
        Self {
            inner: Rc::new(StreamSseScratchInner {
                lane: new_stream_output_lane_cell(),
                accum: PerStreamAccum::new_rc(),
                assistant_message_id: RefCell::new(initial_asst_id),
                post_tool_stream_tail: Cell::new(false),
                pending_tool_message_ids: Rc::new(RefCell::new(VecDeque::new())),
            }),
        }
    }

    #[inline]
    pub(super) fn lane(&self) -> StreamOutputLaneCell {
        Rc::clone(&self.inner.lane)
    }

    #[inline]
    pub(super) fn accum(&self) -> Rc<PerStreamAccum> {
        Rc::clone(&self.inner.accum)
    }

    #[inline]
    pub(super) fn borrow_assistant_id(&self) -> Ref<'_, String> {
        self.inner.assistant_message_id.borrow()
    }

    #[inline]
    pub(super) fn clone_assistant_id(&self) -> String {
        self.inner.assistant_message_id.borrow().clone()
    }

    #[inline]
    pub(super) fn replace_assistant_id(&self, id: String) {
        self.inner.assistant_message_id.replace(id);
    }

    #[inline]
    pub(super) fn post_tool_stream_tail_cell(&self) -> &Cell<bool> {
        &self.inner.post_tool_stream_tail
    }

    /// 与 `on_tool_result` / `on_tool_call` 共用队列句柄。
    #[inline]
    pub(super) fn pending_tool_message_ids(&self) -> Rc<RefCell<VecDeque<String>>> {
        Rc::clone(&self.inner.pending_tool_message_ids)
    }
}
