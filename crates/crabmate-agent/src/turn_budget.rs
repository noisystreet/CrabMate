//! 单轮墙钟与 LLM 调用预算：**`AgentConfig::turn_budget`** 与各处编排共享同一判定与面向用户的文案，
//! 避免 `agent_turn`、`staged/`、`hierarchy` Operator ReAct 各自分叉。
//!
//! 会话侧 **messages 裁剪**仍由 **`context_window` / `message_pipeline`** 负责；本模块表达「本轮是否超墙钟 / 超 LLM 次数」。

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use crabmate_config::TurnBudgetConfig;

/// 与历史 `outer_loop` 安全上限一致；`0` 表示不限制 LLM 调用次数。
pub const DEFAULT_MAX_LLM_CALLS_PER_TURN: u32 = 500;

/// 与历史 `outer_loop` 迭代安全上限一致；`0` 表示不限制外循环轮次。
pub const DEFAULT_MAX_OUTER_LOOP_ITERATIONS: u32 = 500;

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

/// 超 LLM 调用次数时的用户可见短消息。
#[inline]
pub fn turn_llm_calls_limit_user_message(max_llm_calls: u32) -> String {
    format!("已达到单轮 LLM 调用次数上限 ({max_llm_calls})")
}

/// 外循环迭代超上限时的用户可见短消息。
#[inline]
pub fn turn_outer_loop_iterations_limit_user_message(max_iterations: u32) -> String {
    format!("达到外层循环安全上限（{max_iterations} 轮），已中止以避免重复工具调用死循环")
}

/// 单轮共享预算计数（`Arc` 供分层并行子任务与外循环共用）。
#[derive(Debug)]
pub struct TurnBudgetCounter {
    started_at: Instant,
    llm_calls: AtomicU32,
    outer_loop_iterations: AtomicU32,
}

impl TurnBudgetCounter {
    #[inline]
    pub fn new_shared() -> Arc<Self> {
        Arc::new(Self {
            started_at: Instant::now(),
            llm_calls: AtomicU32::new(0),
            outer_loop_iterations: AtomicU32::new(0),
        })
    }

    #[inline]
    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// 记录一次 LLM 调用；返回记录后的累计次数。
    #[inline]
    pub fn record_llm_call(&self) -> u32 {
        self.llm_calls.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[inline]
    pub fn llm_calls(&self) -> u32 {
        self.llm_calls.load(Ordering::Relaxed)
    }

    /// 记录一次外循环迭代；返回记录后的累计次数。
    #[inline]
    pub fn record_outer_loop_iteration(&self) -> u32 {
        self.outer_loop_iterations.fetch_add(1, Ordering::Relaxed) + 1
    }

    #[inline]
    pub fn outer_loop_iterations(&self) -> u32 {
        self.outer_loop_iterations.load(Ordering::Relaxed)
    }

    /// 墙钟是否已超配置上限。
    #[inline]
    pub fn wall_clock_exceeded(&self, cfg: &TurnBudgetConfig) -> bool {
        turn_wall_clock_exceeded(cfg.max_turn_duration_seconds, self.elapsed_secs())
    }

    /// 累计 LLM 调用是否已超上限（`max_llm_calls_per_turn == 0` 表示不限制）。
    #[inline]
    pub fn llm_calls_exceeded(&self, max_llm_calls_per_turn: u32) -> bool {
        max_llm_calls_per_turn > 0 && self.llm_calls() >= max_llm_calls_per_turn
    }

    /// 外循环迭代是否已超上限（`max_outer_loop_iterations == 0` 表示不限制）。
    #[inline]
    pub fn outer_loop_iterations_exceeded(&self, max_outer_loop_iterations: u32) -> bool {
        max_outer_loop_iterations > 0 && self.outer_loop_iterations() >= max_outer_loop_iterations
    }
}

/// 解析有效 LLM 调用上限（配置为 0 时回退默认常量）。
#[inline]
pub fn effective_max_llm_calls_per_turn(cfg: &TurnBudgetConfig) -> u32 {
    if cfg.max_llm_calls_per_turn == 0 {
        DEFAULT_MAX_LLM_CALLS_PER_TURN
    } else {
        cfg.max_llm_calls_per_turn
    }
}

/// 解析有效外循环迭代上限（配置为 0 时回退默认常量）。
#[inline]
pub fn effective_max_outer_loop_iterations(cfg: &TurnBudgetConfig) -> u32 {
    if cfg.max_outer_loop_iterations == 0 {
        DEFAULT_MAX_OUTER_LOOP_ITERATIONS
    } else {
        cfg.max_outer_loop_iterations
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_means_unlimited_wall_clock() {
        assert!(!turn_wall_clock_exceeded(0, 999_999));
    }

    #[test]
    fn exceeded_when_over_cap() {
        assert!(!turn_wall_clock_exceeded(60, 60));
        assert!(turn_wall_clock_exceeded(60, 61));
    }

    #[test]
    fn counter_records_llm_and_iterations() {
        let c = TurnBudgetCounter::new_shared();
        assert_eq!(c.record_llm_call(), 1);
        assert_eq!(c.record_llm_call(), 2);
        assert_eq!(c.record_outer_loop_iteration(), 1);
    }
}
