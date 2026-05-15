//! `/chat/stream` 壳层 **`status_busy` / `tool_busy`** 的显式迁移：由 attach、SSE 回调、HTTP 收尾、
//! 用户中止等路径**只**通过 [`StreamControlSignals::apply_busy_op`](crate::app::app_signals::StreamControlSignals::apply_busy_op) 写入，避免散落 `.set` 与双布尔漂移。
//!
//! 语义与 `chat_session_state::make_chat_stream_busy_memos` 注释对齐；**不**替代会话内 Loading 占位谓词。

use leptos::prelude::{GetUntracked, Set};

use super::app_signals::StreamControlSignals;

/// 壳层「状态栏忙」信号的一次迁移（状态机边，而非整轮流式 FSM）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamShellBusyOp {
    /// 发起新一轮流式前：进入「模型生成中」。
    EnterStreamingStatus,
    /// `on_tool_call` 乐观置位，或 `on_tool_status` 镜像后端 `tool_running`。
    MirrorToolRunning(bool),
    /// `timeline_log` `final_response`：时间轴宣告主答复结束，释放生成中位；工具可能仍忙。
    ReleaseStreamingStatusAfterTimelineFinal,
    /// `on_done` / `on_error` / `on_stream_ended`、HTTP 读完、用户中止：双忙清零。
    ReleaseTurnShellBusy,
}

pub(crate) fn apply_stream_shell_busy_effect(
    op: StreamShellBusyOp,
    status_busy: &mut bool,
    tool_busy: &mut bool,
) {
    match op {
        StreamShellBusyOp::EnterStreamingStatus => *status_busy = true,
        StreamShellBusyOp::MirrorToolRunning(b) => *tool_busy = b,
        StreamShellBusyOp::ReleaseStreamingStatusAfterTimelineFinal => *status_busy = false,
        StreamShellBusyOp::ReleaseTurnShellBusy => {
            *status_busy = false;
            *tool_busy = false;
        }
    }
}

impl StreamControlSignals {
    pub(crate) fn apply_busy_op(&self, op: StreamShellBusyOp) {
        let mut s = self.status_busy.get_untracked();
        let mut t = self.tool_busy.get_untracked();
        apply_stream_shell_busy_effect(op, &mut s, &mut t);
        self.status_busy.set(s);
        self.tool_busy.set(t);
    }

    /// 回落壳层整轮双忙，并在 attach 代际仍匹配时结束 [`super::stream_run_phase::StreamRunPhase::Running`]。
    pub(crate) fn apply_release_turn_and_stream_run(&self, attach_generation: u64) {
        self.end_stream_run_if_current(attach_generation);
        self.apply_busy_op(StreamShellBusyOp::ReleaseTurnShellBusy);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enter_then_release_turn_clears() {
        let mut s = false;
        let mut t = false;
        apply_stream_shell_busy_effect(StreamShellBusyOp::EnterStreamingStatus, &mut s, &mut t);
        assert!(s);
        assert!(!t);
        apply_stream_shell_busy_effect(StreamShellBusyOp::MirrorToolRunning(true), &mut s, &mut t);
        assert!(s);
        assert!(t);
        apply_stream_shell_busy_effect(StreamShellBusyOp::ReleaseTurnShellBusy, &mut s, &mut t);
        assert!(!s);
        assert!(!t);
    }

    #[test]
    fn timeline_final_drops_status_keeps_tool() {
        let mut s = true;
        let mut t = true;
        apply_stream_shell_busy_effect(
            StreamShellBusyOp::ReleaseStreamingStatusAfterTimelineFinal,
            &mut s,
            &mut t,
        );
        assert!(!s);
        assert!(t);
    }
}
