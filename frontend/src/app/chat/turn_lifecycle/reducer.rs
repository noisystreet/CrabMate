//! 纯归约：[`TurnLifecycleState`] + [`apply_turn_lifecycle`]（可单测；Leptos 信号在 [`StreamControlSignals`] 侧 dispatch）。

use super::super::composer_stream::StreamControlEvent;

/// 粗粒度回合阶段（与 [`crate::app::stream_run_phase::StreamRunPhase`] 对齐，后续可合并）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum TurnPhase {
    #[default]
    Idle,
    Attaching {
        attach_generation: u64,
    },
    Streaming {
        attach_generation: u64,
        sub: StreamSubPhase,
    },
    Draining {
        attach_generation: u64,
    },
    Terminal {
        attach_generation: u64,
        outcome: TurnOutcome,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamSubPhase {
    ModelOutput,
    ToolUiBusy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnOutcome {
    Done,
    UserAbort,
    Error,
}

/// 由各 attach / SSE / HTTP / 壳层路径喂入。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TurnLifecycleEvent {
    AttachPrepared { attach_generation: u64 },
    HttpStreamOpened { attach_generation: u64 },
    SseControl(StreamControlEvent),
    ShellReleased { attach_generation: u64 },
    UserAbortRequested { attach_generation: u64 },
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TurnLifecycleState {
    pub phase: TurnPhase,
    tool_depth: u32,
    tool_running_sse: bool,
    stream_ended_seen: bool,
    saw_model_output: bool,
    terminated: bool,
}

impl TurnLifecycleState {
    #[must_use]
    pub(crate) fn current_attach_generation(self) -> Option<u64> {
        match self.phase {
            TurnPhase::Idle => None,
            TurnPhase::Attaching { attach_generation }
            | TurnPhase::Streaming {
                attach_generation, ..
            }
            | TurnPhase::Draining { attach_generation }
            | TurnPhase::Terminal {
                attach_generation, ..
            } => Some(attach_generation),
        }
    }

    fn tool_ui_active(self) -> bool {
        self.tool_depth > 0 || self.tool_running_sse
    }

    fn recompute_streaming_sub(self) -> StreamSubPhase {
        if self.tool_ui_active() {
            StreamSubPhase::ToolUiBusy
        } else {
            StreamSubPhase::ModelOutput
        }
    }

    fn enter_streaming(self, attach_generation: u64) -> TurnPhase {
        TurnPhase::Streaming {
            attach_generation,
            sub: self.recompute_streaming_sub(),
        }
    }

    fn recompute_after_sse(self, attach_generation: u64) -> TurnPhase {
        if self.terminated {
            return TurnPhase::Terminal {
                attach_generation,
                outcome: TurnOutcome::Done,
            };
        }
        if self.stream_ended_seen {
            return TurnPhase::Draining { attach_generation };
        }
        if self.saw_model_output || self.tool_ui_active() {
            return self.enter_streaming(attach_generation);
        }
        TurnPhase::Attaching { attach_generation }
    }
}

/// 与 [`crate::app::stream_run_phase::StreamRunPhase::Running`] 对齐的粗 busy（不含 loading 占位 / abort 槽）。
#[must_use]
pub(crate) fn turn_lifecycle_coarse_busy(state: TurnLifecycleState) -> bool {
    matches!(
        state.phase,
        TurnPhase::Attaching { .. } | TurnPhase::Streaming { .. } | TurnPhase::Draining { .. }
    )
}

/// 状态栏「模型生成中」：Attaching / Draining / 非工具子阶段的 Streaming。
#[must_use]
pub(crate) fn turn_lifecycle_model_ui_busy(state: TurnLifecycleState) -> bool {
    matches!(
        state.phase,
        TurnPhase::Attaching { .. }
            | TurnPhase::Draining { .. }
            | TurnPhase::Streaming {
                sub: StreamSubPhase::ModelOutput,
                ..
            }
    )
}

/// 状态栏「工具执行中」与时间线门闩：Streaming 且处于 ToolUiBusy 子阶段。
#[must_use]
pub(crate) fn turn_lifecycle_tool_ui_busy(state: TurnLifecycleState) -> bool {
    matches!(
        state.phase,
        TurnPhase::Streaming {
            sub: StreamSubPhase::ToolUiBusy,
            ..
        }
    )
}

/// 合成器 / 停止按钮与 legacy `stream_turn_busy_ui` 对齐（lifecycle + Loading 占位 + abort 槽）。
#[must_use]
pub(crate) fn turn_lifecycle_stream_turn_busy(
    state: TurnLifecycleState,
    has_loading_placeholders: bool,
    abort_present: bool,
) -> bool {
    turn_lifecycle_coarse_busy(state) || has_loading_placeholders || abort_present
}

/// 阶段 B 观测：`Attaching | Streaming | Draining | Terminal` 均视为 inflight（Terminal 极短，下一事件回 Idle）。
#[must_use]
pub(crate) fn turn_lifecycle_ui_inflight(state: TurnLifecycleState) -> bool {
    !matches!(state.phase, TurnPhase::Idle)
}

pub(crate) fn apply_turn_lifecycle(state: &mut TurnLifecycleState, ev: TurnLifecycleEvent) {
    match ev {
        TurnLifecycleEvent::AttachPrepared { attach_generation } => {
            state.phase = TurnPhase::Attaching { attach_generation };
            state.tool_depth = 0;
            state.tool_running_sse = false;
            state.stream_ended_seen = false;
            state.saw_model_output = false;
            state.terminated = false;
        }
        TurnLifecycleEvent::HttpStreamOpened { attach_generation } => {
            if !generation_matches(state, attach_generation) {
                return;
            }
            state.phase = state.recompute_after_sse(attach_generation);
        }
        TurnLifecycleEvent::SseControl(sse) => {
            if state.terminated {
                return;
            }
            let Some(attach_generation) = state.current_attach_generation() else {
                return;
            };
            apply_sse_event(state, sse);
            state.phase = state.recompute_after_sse(attach_generation);
        }
        TurnLifecycleEvent::ShellReleased { attach_generation } => {
            if !generation_matches(state, attach_generation) {
                return;
            }
            state.phase = TurnPhase::Idle;
            state.tool_depth = 0;
            state.tool_running_sse = false;
            state.stream_ended_seen = false;
            state.saw_model_output = false;
            state.terminated = false;
        }
        TurnLifecycleEvent::UserAbortRequested { attach_generation } => {
            if !generation_matches(state, attach_generation) {
                return;
            }
            state.terminated = true;
            state.phase = TurnPhase::Terminal {
                attach_generation,
                outcome: TurnOutcome::UserAbort,
            };
        }
    }
}

fn generation_matches(state: &TurnLifecycleState, attach_generation: u64) -> bool {
    state.current_attach_generation() == Some(attach_generation)
}

fn apply_sse_event(state: &mut TurnLifecycleState, ev: StreamControlEvent) {
    match ev {
        StreamControlEvent::ModelTextDelta | StreamControlEvent::AssistantAnswerPhase => {
            state.saw_model_output = true;
        }
        StreamControlEvent::ToolCallDeclared => {
            state.tool_depth = state.tool_depth.saturating_add(1);
        }
        StreamControlEvent::ToolRunning(b) => {
            state.tool_running_sse = b;
        }
        StreamControlEvent::ToolOutputChunk => {}
        StreamControlEvent::ToolResult => {
            state.tool_depth = state.tool_depth.saturating_sub(1);
        }
        StreamControlEvent::StreamEnded => {
            state.stream_ended_seen = true;
        }
        StreamControlEvent::StreamDone => {
            state.terminated = true;
            if let Some(attach_generation) = state.current_attach_generation() {
                state.phase = TurnPhase::Terminal {
                    attach_generation,
                    outcome: TurnOutcome::Done,
                };
            }
        }
        StreamControlEvent::StreamError => {
            state.terminated = true;
            if let Some(attach_generation) = state.current_attach_generation() {
                state.phase = TurnPhase::Terminal {
                    attach_generation,
                    outcome: TurnOutcome::Error,
                };
            }
        }
        StreamControlEvent::StreamUserAbort => {
            state.terminated = true;
            if let Some(attach_generation) = state.current_attach_generation() {
                state.phase = TurnPhase::Terminal {
                    attach_generation,
                    outcome: TurnOutcome::UserAbort,
                };
            }
        }
    }
}
