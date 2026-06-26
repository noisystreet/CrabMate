//! 单轮墙钟预算：**`AgentConfig::max_turn_duration_seconds`** 与各处编排共享同一判定与面向用户的文案，
//! 避免 `agent_turn`、`staged/`、`hierarchy` Operator ReAct 各自分叉。
//!
//! 会话侧 **messages 裁剪**仍由 **`context_window` / `message_pipeline`** 负责；本模块仅表达「本轮是否超墙钟」。

/// `max_turn_duration_seconds == 0` 表示不限制（与 `run_agent_outer_loop` / 分阶段步循环既有语义一致）。
#[inline]
pub fn turn_wall_clock_exceeded(max_turn_duration_seconds: u64, elapsed_secs: u64) -> bool {
    max_turn_duration_seconds > 0 && elapsed_secs > max_turn_duration_seconds
}

/// 与 **`RunAgentTurnError::TimeLimitExhausted`** / SSE 用户文案对齐的短消息。
#[inline]
pub fn turn_wall_clock_limit_user_message(max_turn_duration_seconds: u64) -> String {
    format!("已达到单轮墙钟时间上限 ({}秒)", max_turn_duration_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_means_unlimited() {
        assert!(!turn_wall_clock_exceeded(0, 999_999));
    }

    #[test]
    fn exceeded_when_over_cap() {
        assert!(!turn_wall_clock_exceeded(60, 60));
        assert!(turn_wall_clock_exceeded(60, 61));
    }
}
