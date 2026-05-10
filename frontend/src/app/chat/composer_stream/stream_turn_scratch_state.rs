//! 单次 `/chat/stream` attach 内 SSE 回调共享的 **lane + 累计 + 尾泡 + 工具 FIFO** 收口。
//!
//! 与 Leptos `RwSignal` 解耦（见 `context::ChatStreamCallbackCtx::scratch`）；**车道转移**与 **尾泡轮换副作用**
//! 经本类型方法集中暴露，`callbacks` 子模块避免直接调用 `stream_turn_state` 的自由函数。
//!
//! # 事件 → 行为（维护时请同步）
//!
//! | 来源 | 方法 | lane / 其它 |
//! |------|------|----------------|
//! | `on_assistant_answer_phase` | `on_assistant_answer_phase` | 见 `stream_turn_state::lane_on_assistant_answer_phase` |
//! | `on_delta`（写入前） | `take_followup_rotation_pending` | 若 `true` 须先轮换尾泡（见 `callbacks::helpers`） |
//! | 用户取消 / `on_done` 早退 | `clear_followup_pending` | 丢弃 PendingFollowup |
//! | 工具后 / 多轮正文 | `adopt_new_assistant_tail_after_rotation` | 更新尾泡 id + `post_tool_stream_tail` |

use std::cell::{Cell, Ref, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use super::per_stream_accum::PerStreamAccum;
use super::stream_turn_state::{
    StreamModelOutputLane, StreamOutputLaneCell, lane_clear_followup_pending,
    lane_on_assistant_answer_phase, lane_take_followup_rotation_pending,
    new_stream_output_lane_cell,
};

/// 工具占位 FIFO：`tool_result` 在缺少 `tool_call_id` 时按队列匹配。
pub(super) fn pending_queue_enqueue(q: &Rc<RefCell<VecDeque<String>>>, id: String) {
    q.borrow_mut().push_back(id);
}

/// 弹出 FIFO 队首（若无则 `None`）。
pub(super) fn pending_queue_take(q: &Rc<RefCell<VecDeque<String>>>) -> Option<String> {
    q.borrow_mut().pop_front()
}

/// 单轮流可变草稿（`Clone` 仅为共享 `Rc` 句柄）。
#[derive(Clone)]
pub(super) struct StreamTurnScratchState {
    lane: StreamOutputLaneCell,
    accum: Rc<PerStreamAccum>,
    assistant_message_id: RefCell<String>,
    post_tool_stream_tail: Cell<bool>,
    pending_tool_message_ids: Rc<RefCell<VecDeque<String>>>,
}

impl StreamTurnScratchState {
    #[must_use]
    pub(super) fn new(initial_asst_id: String) -> Self {
        Self {
            lane: new_stream_output_lane_cell(),
            accum: PerStreamAccum::new_rc(),
            assistant_message_id: RefCell::new(initial_asst_id),
            post_tool_stream_tail: Cell::new(false),
            pending_tool_message_ids: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    #[inline]
    pub(super) fn on_assistant_answer_phase(&self) {
        lane_on_assistant_answer_phase(self.lane.as_ref());
    }

    #[inline]
    pub(super) fn take_followup_rotation_pending(&self) -> bool {
        lane_take_followup_rotation_pending(self.lane.as_ref())
    }

    #[inline]
    pub(super) fn clear_followup_pending(&self) {
        lane_clear_followup_pending(self.lane.as_ref());
    }

    #[inline]
    pub(super) fn current_output_lane(&self) -> StreamModelOutputLane {
        self.lane.get()
    }

    #[inline]
    pub(super) fn accum(&self) -> Rc<PerStreamAccum> {
        Rc::clone(&self.accum)
    }

    #[inline]
    pub(super) fn borrow_assistant_id(&self) -> Ref<'_, String> {
        self.assistant_message_id.borrow()
    }

    #[inline]
    pub(super) fn clone_assistant_id(&self) -> String {
        self.assistant_message_id.borrow().clone()
    }

    #[inline]
    pub(super) fn adopt_new_assistant_tail_after_rotation(&self, id: String) {
        self.assistant_message_id.replace(id);
        self.post_tool_stream_tail.set(true);
    }

    #[inline]
    pub(super) fn post_tool_stream_tail_active(&self) -> bool {
        self.post_tool_stream_tail.get()
    }

    #[inline]
    pub(super) fn pending_tool_ids(&self) -> Rc<RefCell<VecDeque<String>>> {
        Rc::clone(&self.pending_tool_message_ids)
    }

    #[inline]
    pub(super) fn enqueue_pending_tool_message_id(&self, id: String) {
        pending_queue_enqueue(&self.pending_tool_message_ids, id);
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
        let q = s.pending_tool_ids();
        assert_eq!(pending_queue_take(&q).as_deref(), Some("m1"));
        assert_eq!(pending_queue_take(&q).as_deref(), Some("m2"));
        assert_eq!(pending_queue_take(&q), None);
    }
}
