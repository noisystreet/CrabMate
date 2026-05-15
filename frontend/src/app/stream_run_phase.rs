//! `/chat/stream` 整轮 HTTP+SSE 在壳层的**粗粒度运行相**（与 [`super::stream_shell_busy::StreamShellBusyOp`] 的子状态互补）。
//!
//! - [`StreamRunPhase::Idle`]：无「当前代际」上的在途 attach（正常收尾或从未发起）。
//! - [`StreamRunPhase::Running`]：[`super::chat::composer_stream::stream_attach_lifecycle::prepare_stream_attach`] 已成功登记本代代际；仅当 `attach_generation` 与当前相仍一致时，[`transition_end_run_if_current`] / [`super::app_signals::StreamControlSignals::end_stream_run_if_current`] 才会回落到 Idle，避免陈旧 HTTP 收尾误清新一轮。

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum StreamRunPhase {
    #[default]
    Idle,
    Running {
        attach_generation: u64,
    },
}

/// 纯迁移：供 [`super::app_signals::StreamControlSignals::end_stream_run_if_current`] 调用。
pub(crate) fn transition_end_run_if_current(phase: &mut StreamRunPhase, attach_generation: u64) {
    if matches!(
        *phase,
        StreamRunPhase::Running {
            attach_generation: g,
        } if g == attach_generation
    ) {
        *phase = StreamRunPhase::Idle;
    }
}
