//! 单次 `/chat/stream` attach 内 SSE 回调共享句柄：内部为 [`super::stream_turn_scratch_state::StreamTurnScratchState`]，
//! 避免在 `ChatStreamCallbackCtx` 与独立参数间分裂可变状态。
//!
//! 与 [`super::context::ChatStreamCallbackCtx`] 分工：ctx 绑定会话、shell、本 scratch；回合收尾仍经
//! [`super::per_stream_accum::PerStreamAccum::summarize_for_stream_done`]。
//! 与 [`super::stream_control_reducer::StreamControlReducerState`] 分工：后者仅归约粗粒度阶段，不参与正文写入。

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use super::callbacks::BubbleOutputQueue;
use super::per_stream_accum::PerStreamAccum;
use super::stream_control_reducer::{StreamControlEvent, StreamControlReducerState};
use super::stream_turn_scratch_state::StreamTurnScratchState;
use super::stream_turn_state::StreamModelOutputLane;
use super::turn_canonical::{
    IngestFinalResponseOutcome, TurnCanonicalState, make_turn_canonical_cell,
};
use crate::app::app_signals::StreamControlSignals;
use crate::app::chat::turn_lifecycle::TurnLifecycleEvent;
use crate::sse_dispatch::TurnSegmentStartInfo;

/// 单轮流 SSE 回调共享的 lane + 累计 + 尾泡可变状态（`Clone` 仅为传递 `Rc` 句柄）。
#[derive(Clone)]
pub(super) struct StreamSseScratch {
    state: Rc<StreamTurnScratchState>,
    control: Rc<RefCell<StreamControlReducerState>>,
    turn: Rc<RefCell<TurnCanonicalState>>,
    bubble_queue: Rc<RefCell<BubbleOutputQueue>>,
}

impl StreamSseScratch {
    #[must_use]
    pub(super) fn new(initial_asst_id: String) -> Self {
        Self {
            state: Rc::new(StreamTurnScratchState::new(initial_asst_id)),
            control: Rc::new(RefCell::new(StreamControlReducerState::new())),
            turn: make_turn_canonical_cell(),
            bubble_queue: Rc::new(RefCell::new(BubbleOutputQueue)),
        }
    }

    /// 同步 [`super::stream_control_reducer`] 与 [`crate::app::chat::turn_lifecycle`]（须在 `stream_ctx.is_stale()` 为假时调用）。
    #[inline]
    pub(super) fn apply_stream_control_event(
        &self,
        stream: &StreamControlSignals,
        ev: StreamControlEvent,
    ) {
        self.control.borrow_mut().apply(ev);
        stream.dispatch_turn_lifecycle(TurnLifecycleEvent::SseControl(ev));
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
    pub(super) fn enter_commentary_before_tools_lane(&self) {
        self.state.enter_commentary_before_tools_lane();
    }

    #[inline]
    pub(super) fn post_tool_stream_tail_active(&self) -> bool {
        self.state.post_tool_stream_tail_active()
    }

    #[inline]
    pub(super) fn take_pending_tool_fifo_head(&self) -> Option<String> {
        self.state.take_pending_tool_fifo_head()
    }

    #[inline]
    pub(super) fn enqueue_pending_tool_message_id(&self, id: String) {
        self.state.enqueue_pending_tool_message_id(id);
    }

    #[inline]
    pub(super) fn on_turn_segment_start(&self, info: TurnSegmentStartInfo) {
        self.turn.borrow_mut().on_segment_start(info);
    }

    #[inline]
    pub(super) fn on_turn_segment_end(&self, segment_id: String) {
        self.turn.borrow_mut().on_segment_end(segment_id);
    }

    #[inline]
    pub(super) fn on_turn_tool_phase_end(&self) {
        self.turn.borrow_mut().on_tool_phase_end();
        self.state.close_post_tool_final_answer_gate();
    }

    #[inline]
    pub(super) fn on_turn_tool_call(&self, tool_call_id: &str, name: &str, summary: &str) {
        self.turn
            .borrow_mut()
            .on_tool_call(tool_call_id, name, summary);
    }

    /// 若 delta 写入 commentary 段（含晚于 `tool_call` 的 plain 增量）则返回 `true`。
    #[inline]
    pub(super) fn try_apply_commentary_delta(&self, delta: &str) -> bool {
        self.turn.borrow_mut().try_apply_commentary_delta(delta)
    }

    /// post-tool 终答 plain delta → canonical `final_answer`。
    #[inline]
    pub(super) fn try_apply_answer_delta(&self, delta: &str) -> bool {
        self.turn.borrow_mut().try_apply_answer_delta(delta)
    }

    pub(super) fn batch_narration_char_len(&self) -> usize {
        self.turn.borrow().batch_narration_char_len()
    }

    #[inline]
    pub(super) fn tool_phase_open(&self) -> bool {
        self.turn.borrow().tool_phase_open()
    }

    /// `final_response` 时间线：阶段 2 起 canonical 不再被该路径写入，由调用方据返回值写 overlay。
    #[inline]
    pub(super) fn try_ingest_final_response_text(
        &self,
        text: &str,
        current_overlay_answer: Option<&str>,
    ) -> IngestFinalResponseOutcome {
        self.turn
            .borrow_mut()
            .try_ingest_final_response_text(text, current_overlay_answer)
    }

    #[inline]
    pub(super) fn absorb_pre_tool_narration_for_first_tool(&self, from_bubble: &str) {
        self.turn
            .borrow_mut()
            .absorb_pre_tool_narration_for_first_tool(from_bubble);
    }

    #[inline]
    pub(super) fn ingest_batch_commentary_from_peel(&self, text: &str) {
        self.turn
            .borrow_mut()
            .ingest_batch_commentary_from_peel(text);
    }

    #[inline]
    pub(super) fn close_post_tool_final_answer_gate(&self) {
        self.state.close_post_tool_final_answer_gate();
    }

    #[inline]
    pub(super) fn post_tool_final_answer_open(&self) -> bool {
        self.state.post_tool_final_answer_open()
    }

    #[inline]
    pub(super) fn open_post_tool_final_answer_gate(&self) {
        self.state.open_post_tool_final_answer_gate();
    }

    pub(super) fn repartition_turn_for_web_layout(&self) {
        self.turn.borrow_mut().repartition_web_block_layout_stream();
        self.state.open_post_tool_final_answer_gate();
    }

    /// 流结束：关 open 段、尾泡正文入 canonical 并投影落盘。
    pub(super) fn finalize_turn_projection_before_stream_done(
        &self,
        stream_ctx: &super::context::ChatStreamCallbackCtx,
    ) {
        super::callbacks::TurnLayout::finalize_turn_projection_before_stream_done(stream_ctx);
    }

    #[inline]
    pub(super) fn close_open_commentary_for_projection(&self) {
        self.turn
            .borrow_mut()
            .close_open_commentary_for_projection();
    }

    /// delta 热路径：仅更新 loading 尾泡 preview，说明块经 [`Self::sync_turn_projection`] 落盘。
    pub(super) fn sync_stream_preview(&self, stream_ctx: &super::context::ChatStreamCallbackCtx) {
        let turn = self.turn.borrow();
        super::callbacks::TurnLayout::sync_stream_preview(stream_ctx, &turn);
    }

    /// 段/工具边界：flush 工具批说明块到 stored。
    pub(super) fn sync_turn_projection(&self, stream_ctx: &super::context::ChatStreamCallbackCtx) {
        let turn = self.turn.borrow();
        let mut queue = self.bubble_queue.borrow_mut();
        super::callbacks::TurnLayout::sync_turn_projection(stream_ctx, &turn, &mut queue);
    }

    /// 新模型轮次：清空 canonical 终答桶，避免旧文本覆盖新气泡 overlay。
    pub(super) fn reset_canonical_final_answer_for_new_round(&self) {
        self.turn.borrow_mut().reset_final_answer_for_new_round();
    }
}
