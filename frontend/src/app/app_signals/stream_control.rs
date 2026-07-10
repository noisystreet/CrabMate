//! 流式回合忙状态、中止槽与 [`TurnLifecycleState`] 归约。

use std::sync::{Arc, Mutex};

use leptos::prelude::*;

use crate::app::turn_lifecycle::{TurnLifecycleEvent, TurnLifecycleState, apply_turn_lifecycle};

#[derive(Clone)]
pub struct StreamControlSignals {
    pub status_err: RwSignal<Option<String>>,
    /// 单轮 `/chat/stream` 粗粒度生命周期；UI 门闩与 attach 代际均读此信号。
    pub turn_lifecycle: RwSignal<TurnLifecycleState>,
    /// `AbortController` 槽位在 `Mutex` 中，Leptos 无法订阅；每次槽位变更时递增，供
    /// [`crate::chat_session_state::make_chat_stream_busy_memos`] 与「整轮 UI 忙」`Memo` 失效并重算。
    pub stream_abort_epoch: RwSignal<u32>,
    pub abort_cell: Arc<Mutex<Option<web_sys::AbortController>>>,
    pub user_cancelled_stream: Arc<Mutex<bool>>,
}

impl StreamControlSignals {
    pub fn new() -> Self {
        Self {
            status_err: RwSignal::new(None),
            turn_lifecycle: RwSignal::new(TurnLifecycleState::default()),
            stream_abort_epoch: RwSignal::new(0),
            abort_cell: Arc::new(Mutex::new(None)),
            user_cancelled_stream: Arc::new(Mutex::new(false)),
        }
    }

    pub(crate) fn dispatch_turn_lifecycle(&self, ev: TurnLifecycleEvent) {
        self.turn_lifecycle
            .update(|st| apply_turn_lifecycle(st, ev));
    }

    pub(crate) fn begin_stream_run(&self, attach_generation: u64) {
        self.dispatch_turn_lifecycle(TurnLifecycleEvent::AttachPrepared { attach_generation });
    }

    /// HTTP / SSE / 用户中止等收尾：仅当代际仍匹配时回落 lifecycle 到 Idle。
    pub(crate) fn apply_release_turn_and_stream_run(&self, attach_generation: u64) {
        self.dispatch_turn_lifecycle(TurnLifecycleEvent::ShellReleased { attach_generation });
    }
}

impl Default for StreamControlSignals {
    fn default() -> Self {
        Self::new()
    }
}
