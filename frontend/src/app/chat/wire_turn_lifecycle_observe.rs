//! 阶段 B：[`TurnLifecycleState`] 与 legacy `StreamRunPhase` 双写观测（仅 `debug_assertions`）。

use leptos::prelude::*;

use crate::app::app_signals::StreamControlSignals;
use crate::app::chat::turn_lifecycle::{turn_lifecycle_coarse_busy, turn_lifecycle_ui_inflight};
use crate::app::stream_run_phase::StreamRunPhase;

/// 订阅 lifecycle 与 `stream_run_phase`；开发构建下不一致时 `debug_assert` 失败。
pub(crate) fn wire_turn_lifecycle_observe(stream: StreamControlSignals) {
    #[cfg(debug_assertions)]
    Effect::new(move |_| {
        let lc_busy = turn_lifecycle_coarse_busy(stream.turn_lifecycle.get());
        let _lc_inflight = turn_lifecycle_ui_inflight(stream.turn_lifecycle.get());
        let run_busy = matches!(
            stream.stream_run_phase.get(),
            StreamRunPhase::Running { .. }
        );
        debug_assert!(
            lc_busy == run_busy,
            "turn_lifecycle coarse_busy ({lc_busy}) != stream_run_phase Running ({run_busy})"
        );
    });
}
