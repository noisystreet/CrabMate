//! 单次 `/chat/stream` attach 内 SSE 回调共享的 **lane + 累计 + 尾泡 + 工具 FIFO** 收口。
//!
//! 与 Leptos `RwSignal` 解耦（见 `context::ChatStreamCallbackCtx::scratch`）；**车道转移**与 **尾泡轮换副作用**
//! 经本类型方法集中暴露，`callbacks` 子模块避免直接调用 `stream_turn_state` 的自由函数。
//!
//! # 事件 → 行为（维护时请同步）
//!
//! | 来源 | 方法 | lane / 其它 |
//! |------|------|----------------|
//! | `on_assistant_answer_phase` | `on_assistant_answer_phase` | [`StreamModelOutputLane::apply_assistant_answer_phase`] |
//! | `on_delta`（写入前） | `take_followup_rotation_pending` | 若 `true` 须先轮换尾泡（见 `callbacks::helpers`） |
//! | 用户取消 / `on_done` 早退 | `clear_followup_pending` | 丢弃 PendingFollowup |
//! | 工具后 / 多轮正文 | `adopt_new_assistant_tail_after_rotation` | 更新尾泡 id + `post_tool_stream_tail` |
//!
//! # 单 `RefCell` 内聚
//!
//! [`StreamTurnScratchInner`] 将 **lane + 尾泡 id + post_tool 标记 + FIFO** 收进一处可变快照，由 **单个**
//! [`RefCell`] 保护，避免多 `Cell`/`RefCell` 交叉借用；车道语义见 [`super::stream_turn_state::StreamModelOutputLane`]。

use std::cell::{Ref, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use super::per_stream_accum::PerStreamAccum;
use super::stream_turn_state::StreamModelOutputLane;

/// 单轮流可变快照（仅由 [`StreamTurnScratchState`] 的 `RefCell` 持有）。
struct StreamTurnScratchInner {
    lane: StreamModelOutputLane,
    assistant_message_id: String,
    post_tool_stream_tail: bool,
    pending_tool_message_ids: VecDeque<String>,
}

impl StreamTurnScratchInner {
    fn new(initial_asst_id: String) -> Self {
        Self {
            lane: StreamModelOutputLane::default(),
            assistant_message_id: initial_asst_id,
            post_tool_stream_tail: false,
            pending_tool_message_ids: VecDeque::new(),
        }
    }
}

/// 单轮流可变草稿（`Clone` 仅为共享 `Rc` 句柄）。
#[derive(Clone)]
pub(super) struct StreamTurnScratchState {
    inner: Rc<RefCell<StreamTurnScratchInner>>,
    accum: Rc<PerStreamAccum>,
}

impl StreamTurnScratchState {
    #[must_use]
    pub(super) fn new(initial_asst_id: String) -> Self {
        Self {
            inner: Rc::new(RefCell::new(StreamTurnScratchInner::new(initial_asst_id))),
            accum: PerStreamAccum::new_rc(),
        }
    }

    #[inline]
    pub(super) fn on_assistant_answer_phase(&self) {
        self.inner.borrow_mut().lane.apply_assistant_answer_phase();
    }

    #[inline]
    pub(super) fn take_followup_rotation_pending(&self) -> bool {
        self.inner
            .borrow_mut()
            .lane
            .take_followup_rotation_if_pending()
    }

    #[inline]
    pub(super) fn clear_followup_pending(&self) {
        self.inner.borrow_mut().lane.clear_followup_pending_lane();
    }

    #[inline]
    pub(super) fn current_output_lane(&self) -> StreamModelOutputLane {
        self.inner.borrow().lane
    }

    #[inline]
    pub(super) fn accum(&self) -> Rc<PerStreamAccum> {
        Rc::clone(&self.accum)
    }

    #[inline]
    pub(super) fn borrow_assistant_id(&self) -> Ref<'_, String> {
        Ref::map(self.inner.borrow(), |i| &i.assistant_message_id)
    }

    #[inline]
    pub(super) fn clone_assistant_id(&self) -> String {
        self.inner.borrow().assistant_message_id.clone()
    }

    #[inline]
    pub(super) fn adopt_new_assistant_tail_after_rotation(&self, id: String) {
        let mut g = self.inner.borrow_mut();
        g.assistant_message_id = id;
        g.post_tool_stream_tail = true;
    }

    #[inline]
    pub(super) fn post_tool_stream_tail_active(&self) -> bool {
        self.inner.borrow().post_tool_stream_tail
    }

    /// 工具占位 FIFO：弹出队首（`tool_result` 在缺少 `tool_call_id` 时按队列匹配）。
    #[inline]
    pub(super) fn take_pending_tool_fifo_head(&self) -> Option<String> {
        self.inner.borrow_mut().pending_tool_message_ids.pop_front()
    }

    #[inline]
    pub(super) fn enqueue_pending_tool_message_id(&self, id: String) {
        self.inner
            .borrow_mut()
            .pending_tool_message_ids
            .push_back(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_phase_then_take_pending_matches_stream_turn_state() {
        let s = StreamTurnScratchState::new("a1".into());
        assert_eq!(s.current_output_lane(), StreamModelOutputLane::Reasoning);
        s.on_assistant_answer_phase();
        assert_eq!(s.current_output_lane(), StreamModelOutputLane::Answering);
        s.on_assistant_answer_phase();
        assert_eq!(
            s.current_output_lane(),
            StreamModelOutputLane::AnsweringPendingFollowupBubble
        );
        assert!(s.take_followup_rotation_pending());
        assert_eq!(s.current_output_lane(), StreamModelOutputLane::Answering);
        assert!(!s.take_followup_rotation_pending());
    }

    #[test]
    fn clear_followup_pending_from_pending_state() {
        let s = StreamTurnScratchState::new("a1".into());
        s.on_assistant_answer_phase();
        s.on_assistant_answer_phase();
        assert_eq!(
            s.current_output_lane(),
            StreamModelOutputLane::AnsweringPendingFollowupBubble
        );
        s.clear_followup_pending();
        assert_eq!(s.current_output_lane(), StreamModelOutputLane::Answering);
    }

    #[test]
    fn adopt_tail_sets_id_and_post_tool_flag() {
        let s = StreamTurnScratchState::new("old".into());
        assert!(!s.post_tool_stream_tail_active());
        s.adopt_new_assistant_tail_after_rotation("new".into());
        assert_eq!(s.clone_assistant_id(), "new");
        assert!(s.post_tool_stream_tail_active());
    }

    #[test]
    fn pending_fifo_enqueue_take() {
        let s = StreamTurnScratchState::new("x".into());
        s.enqueue_pending_tool_message_id("m1".into());
        s.enqueue_pending_tool_message_id("m2".into());
        assert_eq!(s.take_pending_tool_fifo_head().as_deref(), Some("m1"));
        assert_eq!(s.take_pending_tool_fifo_head().as_deref(), Some("m2"));
        assert_eq!(s.take_pending_tool_fifo_head(), None);
    }
}
