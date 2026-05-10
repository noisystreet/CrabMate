//! 单次 `/chat/stream` attach 内 SSE 回调共享句柄：内部为 [`super::stream_turn_scratch_state::StreamTurnScratchState`]，
//! 避免在 `ChatStreamCallbackCtx` 与独立参数间分裂可变状态。
//!
//! 与 [`super::context::ChatStreamCallbackCtx`] 分工：ctx 绑定会话、shell、本 scratch；回合收尾仍经
//! [`super::per_stream_accum::PerStreamAccum::summarize_for_stream_done`]。

use std::cell::{Ref, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use super::per_stream_accum::PerStreamAccum;
use super::stream_turn_scratch_state::StreamTurnScratchState;
use super::stream_turn_state::StreamModelOutputLane;

/// 单轮流 SSE 回调共享的 lane + 累计 + 尾泡可变状态（`Clone` 仅为传递 `Rc` 句柄）。
#[derive(Clone)]
pub(super) struct StreamSseScratch {
    state: Rc<StreamTurnScratchState>,
}

impl StreamSseScratch {
    #[must_use]
    pub(super) fn new(initial_asst_id: String) -> Self {
        Self {
            state: Rc::new(StreamTurnScratchState::new(initial_asst_id)),
        }
    }

    #[inline]
    pub(super) fn on_assistant_answer_phase(&self) {
        self.state.on_assistant_answer_phase();
    }

    #[inline]
    pub(super) fn take_followup_rotation_pending(&self) -> bool {
        self.state.take_followup_rotation_pending()
    }

    #[inline]
    pub(super) fn clear_followup_pending(&self) {
        self.state.clear_followup_pending();
    }

    #[inline]
    pub(super) fn current_output_lane(&self) -> StreamModelOutputLane {
        self.state.current_output_lane()
    }

    #[inline]
    pub(super) fn accum(&self) -> Rc<PerStreamAccum> {
        self.state.accum()
    }

    #[inline]
    pub(super) fn borrow_assistant_id(&self) -> Ref<'_, String> {
        self.state.borrow_assistant_id()
    }

    #[inline]
    pub(super) fn clone_assistant_id(&self) -> String {
        self.state.clone_assistant_id()
    }

    #[inline]
    pub(super) fn adopt_new_assistant_tail_after_rotation(&self, id: String) {
        self.state.adopt_new_assistant_tail_after_rotation(id);
    }

    #[inline]
    pub(super) fn post_tool_stream_tail_active(&self) -> bool {
        self.state.post_tool_stream_tail_active()
    }

    #[inline]
    pub(super) fn pending_tool_message_ids(&self) -> Rc<RefCell<VecDeque<String>>> {
        self.state.pending_tool_ids()
    }

    #[inline]
    pub(super) fn enqueue_pending_tool_message_id(&self, id: String) {
        self.state.enqueue_pending_tool_message_id(id);
    }
}
