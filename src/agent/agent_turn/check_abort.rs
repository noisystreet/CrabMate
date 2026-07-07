//! SSE 断开 / 用户取消的快速检查宏，收敛 `sse_sender_closed` + `cancel` 重复模式。
//!
//! 在 P/R/E 等各阶段切换处使用，避免遗忘检查或分散的守卫逻辑。

/// 检查 SSE 通道是否关闭或用户是否取消，若是则返回 `Err(RunAgentTurnError::TurnAborted)`。
///
/// # 用法
/// ```ignore
/// check_abort!(ctx.io, AgentTurnSubPhase::Planner);
/// // 后面的代码只有未取消时才会执行
/// ```
#[macro_export]
macro_rules! check_abort {
    ($io:expr, $phase:expr) => {{
        let io = &$io;
        let phase: $crate::agent::agent_turn::errors::AgentTurnSubPhase = $phase;
        if $crate::agent::agent_turn::execute_tools::sse_sender_closed(io.out) {
            return Err(
                $crate::agent::agent_turn::errors::RunAgentTurnError::TurnAborted {
                    phase,
                    reason: $crate::agent::agent_turn::errors::TurnAbortReason::SseDisconnected,
                },
            );
        }
        if io
            .cancel
            .is_some_and(|c| c.load(std::sync::atomic::Ordering::SeqCst))
        {
            return Err(
                $crate::agent::agent_turn::errors::RunAgentTurnError::TurnAborted {
                    phase,
                    reason: $crate::agent::agent_turn::errors::TurnAbortReason::UserCancelled,
                },
            );
        }
    }};
}
