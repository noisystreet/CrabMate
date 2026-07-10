//! 阶段 C：[`TurnLifecycleState`] 与 legacy `StreamRunPhase` 双写观测（`debug_assertions`）。

use leptos::prelude::*;

use crate::app::app_signals::StreamControlSignals;
use crate::app::stream_run_phase::StreamRunPhase;
use crate::app::turn_lifecycle::{
    turn_lifecycle_coarse_busy, turn_lifecycle_model_ui_busy, turn_lifecycle_tool_ui_busy,
    turn_lifecycle_ui_inflight,
};

/// 订阅 lifecycle 与 `stream_run_phase`；开发构建下不一致时 `debug_assert` 失败。
pub(crate) fn wire_turn_lifecycle_observe(stream: StreamControlSignals) {
    #[cfg(debug_assertions)]
    Effect::new(move |_| {
        let lc = stream.turn_lifecycle.get();
        let lc_busy = turn_lifecycle_coarse_busy(lc);
        let _lc_inflight = turn_lifecycle_ui_inflight(lc);
        let _model_busy = turn_lifecycle_model_ui_busy(lc);
        let _tool_busy = turn_lifecycle_tool_ui_busy(lc);
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
