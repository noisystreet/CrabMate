//! 单次 `/chat/stream` attach 内 **控制面 + 正文增量** 的粗粒度阶段（与 `dispatch.rs` 解耦）。
//!
//! 阶段 A：**观测与单测锁行为**——不改变既有 UI 副作用；[`StreamControlReducerState::apply`]
//! 仅在各 `on_*` 路径上同步计数/标志，供调试与后续收紧不变量。
//!
//! 与 [`super::stream_turn_state::StreamModelOutputLane`] 正交：lane 管 reasoning/answer 写入；
//! 本模块管「工具线是否占用」「是否已见模型输出」「是否已进入 drain/terminal」。

/// 与 UI 壳层 `stream_run_phase` 不同：仅描述 **本轮 SSE attach** 的消费进度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StreamControlPhase {
    /// 尚未收到正文 delta / answer_phase / 工具声明。
    Idle,
    /// 已出现模型输出或曾处于工具线，且未 `stream_ended`、未终止。
    Streaming,
    /// `tool_call` 未配对完 `tool_result`，或最近 SSE `tool_running=true`。
    ToolUiBusy,
    /// 已收到 `stream_ended`，等待 `on_done` 等收尾。
    Draining,
    /// `on_done` / `on_error` / 用户中止等之后不再跃迁。
    Terminal,
}

/// 由各 `ChatStreamCallbacks` 路径喂入的语义事件（非原始 JSON）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StreamControlEvent {
    ModelTextDelta,
    AssistantAnswerPhase,
    ToolCallDeclared,
    ToolRunning(bool),
    ToolOutputChunk,
    ToolResult,
    StreamEnded,
    /// 正常 `on_done` 走完（含尾泡决策后）。
    StreamDone,
    StreamError,
    /// `on_done` 早退：用户取消本轮（与 `StreamDone` 同属会话收口）。
    StreamUserAbort,
}

#[derive(Debug, Clone)]
pub(super) struct StreamControlReducerState {
    phase: StreamControlPhase,
    terminated: bool,
    stream_ended_seen: bool,
    tool_depth: u32,
    tool_running_sse: bool,
    saw_any_model_output: bool,
}

impl StreamControlReducerState {
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            phase: StreamControlPhase::Idle,
            terminated: false,
            stream_ended_seen: false,
            tool_depth: 0,
            tool_running_sse: false,
            saw_any_model_output: false,
        }
    }

    /// 当前粗粒度阶段（单测与后续断言用；普通 `cargo check` 下调用方尚未接入。）
    #[allow(dead_code)]
    #[inline]
    pub(super) fn phase(&self) -> StreamControlPhase {
        self.phase
    }

    #[inline]
    fn tool_ui_active(&self) -> bool {
        self.tool_depth > 0 || self.tool_running_sse
    }

    fn recompute_phase(&self) -> StreamControlPhase {
        if self.terminated {
            return StreamControlPhase::Terminal;
        }
        if self.stream_ended_seen {
            return StreamControlPhase::Draining;
        }
        if self.tool_ui_active() {
            return StreamControlPhase::ToolUiBusy;
        }
        if self.saw_any_model_output {
            return StreamControlPhase::Streaming;
        }
        StreamControlPhase::Idle
    }

    fn sync_phase(&mut self) {
        self.phase = self.recompute_phase();
    }

    /// 应用单条事件并刷新 [`Self::phase`]（`Terminal` 之后为幂等 no-op）。
    pub(super) fn apply(&mut self, ev: StreamControlEvent) {
        if self.terminated {
            return;
        }
        match ev {
            StreamControlEvent::ModelTextDelta | StreamControlEvent::AssistantAnswerPhase => {
                self.saw_any_model_output = true;
            }
            StreamControlEvent::ToolCallDeclared => {
                self.tool_depth = self.tool_depth.saturating_add(1);
            }
            StreamControlEvent::ToolRunning(b) => {
                self.tool_running_sse = b;
            }
            StreamControlEvent::ToolOutputChunk => {}
            StreamControlEvent::ToolResult => {
                self.tool_depth = self.tool_depth.saturating_sub(1);
            }
            StreamControlEvent::StreamEnded => {
                self.stream_ended_seen = true;
            }
            StreamControlEvent::StreamDone
            | StreamControlEvent::StreamError
            | StreamControlEvent::StreamUserAbort => {
                self.terminated = true;
            }
        }
        self.sync_phase();
    }
}

#[cfg(test)]
mod tests {
    use super::{StreamControlEvent, StreamControlPhase, StreamControlReducerState};

    fn apply_seq(s: &mut StreamControlReducerState, evs: &[StreamControlEvent]) {
        for e in evs {
            s.apply(*e);
        }
    }

    #[test]
    fn idle_delta_then_answer_phase_stays_streaming() {
        let mut s = StreamControlReducerState::new();
        apply_seq(
            &mut s,
            &[
                StreamControlEvent::ModelTextDelta,
                StreamControlEvent::AssistantAnswerPhase,
            ],
        );
        assert_eq!(s.phase(), StreamControlPhase::Streaming);
    }

    #[test]
    fn tool_call_then_result_returns_to_streaming() {
        let mut s = StreamControlReducerState::new();
        apply_seq(
            &mut s,
            &[
                StreamControlEvent::ModelTextDelta,
                StreamControlEvent::ToolCallDeclared,
            ],
        );
        assert_eq!(s.phase(), StreamControlPhase::ToolUiBusy);
        s.apply(StreamControlEvent::ToolResult);
        assert_eq!(s.phase(), StreamControlPhase::Streaming);
    }

    #[test]
    fn stream_ended_overrides_tool_busy() {
        let mut s = StreamControlReducerState::new();
        apply_seq(
            &mut s,
            &[
                StreamControlEvent::ToolCallDeclared,
                StreamControlEvent::StreamEnded,
            ],
        );
        assert_eq!(s.phase(), StreamControlPhase::Draining);
    }

    #[test]
    fn draining_then_done_is_terminal() {
        let mut s = StreamControlReducerState::new();
        apply_seq(
            &mut s,
            &[
                StreamControlEvent::AssistantAnswerPhase,
                StreamControlEvent::StreamEnded,
                StreamControlEvent::StreamDone,
            ],
        );
        assert_eq!(s.phase(), StreamControlPhase::Terminal);
    }

    #[test]
    fn tool_running_true_without_call_enters_busy() {
        let mut s = StreamControlReducerState::new();
        s.apply(StreamControlEvent::ToolRunning(true));
        assert_eq!(s.phase(), StreamControlPhase::ToolUiBusy);
        s.apply(StreamControlEvent::ToolRunning(false));
        assert_eq!(s.phase(), StreamControlPhase::Idle);
    }

    #[test]
    fn terminal_ignores_late_tool_events() {
        let mut s = StreamControlReducerState::new();
        apply_seq(&mut s, &[StreamControlEvent::StreamError]);
        assert_eq!(s.phase(), StreamControlPhase::Terminal);
        s.apply(StreamControlEvent::ToolCallDeclared);
        assert_eq!(s.phase(), StreamControlPhase::Terminal);
    }
}
