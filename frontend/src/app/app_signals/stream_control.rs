//! 流式回合忙状态、中止槽与整轮流式相。

use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::app::stream_run_phase::{StreamRunPhase, transition_end_run_if_current};

#[derive(Clone)]
pub struct StreamControlSignals {
    pub status_busy: RwSignal<bool>,
    pub status_err: RwSignal<Option<String>>,
    pub tool_busy: RwSignal<bool>,
    /// 与 `attach_generation` 对齐的整轮流式相（Idle | Running）；见 [`super::stream_run_phase`].
    pub stream_run_phase: RwSignal<StreamRunPhase>,
    /// `AbortController` 槽位在 `Mutex` 中，Leptos 无法订阅；每次槽位变更时递增，供
    /// [`crate::chat_session_state::make_chat_stream_busy_memos`] 与「整轮 UI 忙」`Memo` 失效并重算。
    pub stream_abort_epoch: RwSignal<u32>,
    pub abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub user_cancelled_stream: Arc<Mutex<bool>>,
}

impl StreamControlSignals {
    pub fn new() -> Self {
        Self {
            status_busy: RwSignal::new(false),
            status_err: RwSignal::new(None),
            tool_busy: RwSignal::new(false),
            stream_run_phase: RwSignal::new(StreamRunPhase::Idle),
            stream_abort_epoch: RwSignal::new(0),
            abort_cell: Arc::new(Mutex::new(None)),
            user_cancelled_stream: Arc::new(Mutex::new(false)),
        }
    }

    pub(crate) fn begin_stream_run(&self, attach_generation: u64) {
        self.stream_run_phase
            .set(StreamRunPhase::Running { attach_generation });
    }

    pub(crate) fn end_stream_run_if_current(&self, attach_generation: u64) {
        self.stream_run_phase
            .update(|p| transition_end_run_if_current(p, attach_generation));
    }
}

impl Default for StreamControlSignals {
    fn default() -> Self {
        Self::new()
    }
}
