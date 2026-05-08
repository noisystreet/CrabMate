//! 流式回合内模型输出通道：将 `assistant_answer_phase` 与「多段正文需轮换气泡」收敛为单一枚举，
//! 替代一对交叉读写的 `Cell<bool>`。
//!
//! 与 [`super::stream_sse_scratch::StreamSseScratch`] 同层，供 SSE 回调装配与单轮草稿共用，避免与 `callbacks` 子模块循环依赖。

use std::cell::Cell;
use std::rc::Rc;

/// 当前 `delta` 写入 reasoning 还是正文，以及是否需在下一片段前轮换助手气泡。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum StreamModelOutputLane {
    /// 尚未收到 `assistant_answer_phase`，`delta` 写入 reasoning。
    #[default]
    Reasoning,
    /// 已在正文相；`delta` 写入正文。
    Answering,
    /// 正文相内再次收到 `assistant_answer_phase`：须在下次 `delta` 或 `on_done` 时轮换气泡。
    AnsweringPendingFollowupBubble,
}

impl StreamModelOutputLane {
    #[must_use]
    pub(super) const fn in_answer_body_lane(self) -> bool {
        !matches!(self, Self::Reasoning)
    }
}

pub(crate) type StreamOutputLaneCell = Rc<Cell<StreamModelOutputLane>>;

#[must_use]
pub(crate) fn new_stream_output_lane_cell() -> StreamOutputLaneCell {
    Rc::new(Cell::new(StreamModelOutputLane::default()))
}

/// [`crate::api::ChatStreamCallbacks::on_assistant_answer_phase`]：首次进入正文相，或标记待轮换。
pub(super) fn lane_on_assistant_answer_phase(lane: &Cell<StreamModelOutputLane>) {
    lane.set(match lane.get() {
        StreamModelOutputLane::Reasoning => StreamModelOutputLane::Answering,
        StreamModelOutputLane::Answering
        | StreamModelOutputLane::AnsweringPendingFollowupBubble => {
            StreamModelOutputLane::AnsweringPendingFollowupBubble
        }
    });
}

/// 若处于「待轮换」状态，返回 `true` 并回落到 [`StreamModelOutputLane::Answering`]。
pub(super) fn lane_take_followup_rotation_pending(lane: &Cell<StreamModelOutputLane>) -> bool {
    if matches!(
        lane.get(),
        StreamModelOutputLane::AnsweringPendingFollowupBubble
    ) {
        lane.set(StreamModelOutputLane::Answering);
        true
    } else {
        false
    }
}

/// 用户取消等路径：丢弃「待轮换」，保留是否已在正文相。
pub(super) fn lane_clear_followup_pending(lane: &Cell<StreamModelOutputLane>) {
    if matches!(
        lane.get(),
        StreamModelOutputLane::AnsweringPendingFollowupBubble
    ) {
        lane.set(StreamModelOutputLane::Answering);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_first_answer_phase_enters_answering() {
        let c = Cell::new(StreamModelOutputLane::Reasoning);
        lane_on_assistant_answer_phase(&c);
        assert_eq!(c.get(), StreamModelOutputLane::Answering);
    }

    #[test]
    fn lane_second_answer_phase_marks_pending() {
        let c = Cell::new(StreamModelOutputLane::Answering);
        lane_on_assistant_answer_phase(&c);
        assert_eq!(
            c.get(),
            StreamModelOutputLane::AnsweringPendingFollowupBubble
        );
    }

    #[test]
    fn lane_take_pending_rotates_once() {
        let c = Cell::new(StreamModelOutputLane::AnsweringPendingFollowupBubble);
        assert!(lane_take_followup_rotation_pending(&c));
        assert_eq!(c.get(), StreamModelOutputLane::Answering);
        assert!(!lane_take_followup_rotation_pending(&c));
    }

    #[test]
    fn lane_clear_pending_from_cancel() {
        let c = Cell::new(StreamModelOutputLane::AnsweringPendingFollowupBubble);
        lane_clear_followup_pending(&c);
        assert_eq!(c.get(), StreamModelOutputLane::Answering);
    }
}
