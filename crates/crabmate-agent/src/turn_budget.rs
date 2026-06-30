//! 单轮墙钟、LLM 次数与 Token 粗估预算：**`AgentConfig::turn_budget`** 与各处编排共享同一判定与面向用户的文案，
//! 避免 `agent_turn`、`staged/`、`hierarchy` Operator ReAct 各自分叉。
//!
//! 会话侧 **messages 裁剪**仍由 **`context_window` / `message_pipeline`** 负责；本模块表达「本轮是否超墙钟 / 超 LLM 次数 / 超 Token 粗估」。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
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

/// 超 Token 粗估上限时的用户可见短消息。
#[inline]
pub fn turn_tokens_limit_user_message(max_turn_tokens: usize) -> String {
    format!("已达到单轮 Token 预算上限 (~{max_turn_tokens})")
}

/// 外循环迭代超上限时的用户可见短消息。
#[inline]
pub fn turn_outer_loop_iterations_limit_user_message(max_iterations: u32) -> String {
    format!("达到外层循环安全上限（{max_iterations} 轮），已中止以避免重复工具调用死循环")
}

/// 预算耗尽且存在部分进展时的附注（分层 Operator 等路径）。
#[inline]
pub fn turn_budget_partial_completion_suffix() -> &'static str {
    "（预算已耗尽，以下为已完成部分的摘要）"
}

/// 是否为 [`deny_llm_call_if_exhausted`] 返回的预算门禁文案（供分层 ReAct 优雅收尾）。
#[inline]
pub fn is_turn_budget_limit_user_message(msg: &str) -> bool {
    msg.contains("单轮墙钟时间上限")
        || msg.contains("单轮 LLM 调用次数上限")
        || msg.contains("单轮 Token 预算上限")
}

/// 单轮共享预算计数（`Arc` 供分层并行子任务与外循环共用）。
#[derive(Debug)]
pub struct TurnBudgetCounter {
    started_at: Instant,
    llm_calls: AtomicU32,
    outer_loop_iterations: AtomicU32,
    estimated_tokens: AtomicUsize,
    degradation_active: AtomicBool,
}

impl TurnBudgetCounter {
    #[inline]
    pub fn new_shared() -> Arc<Self> {
        Arc::new(Self {
            started_at: Instant::now(),
            llm_calls: AtomicU32::new(0),
            outer_loop_iterations: AtomicU32::new(0),
            estimated_tokens: AtomicUsize::new(0),
            degradation_active: AtomicBool::new(false),
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

    /// 累加一次 LLM 往返的 prompt+completion Token 粗估。
    #[inline]
    pub fn record_estimated_tokens(&self, tokens: usize) {
        if tokens == 0 {
            return;
        }
        self.estimated_tokens.fetch_add(tokens, Ordering::Relaxed);
    }

    #[inline]
    pub fn estimated_tokens(&self) -> usize {
        self.estimated_tokens.load(Ordering::Relaxed)
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

    /// Token 粗估是否已超上限（`max_turn_tokens == 0` 表示不限制）。
    #[inline]
    pub fn tokens_exceeded(&self, max_turn_tokens: usize) -> bool {
        max_turn_tokens > 0 && self.estimated_tokens() >= max_turn_tokens
    }

    /// 外循环迭代是否已超上限（`max_outer_loop_iterations == 0` 表示不限制）。
    #[inline]
    pub fn outer_loop_iterations_exceeded(&self, max_outer_loop_iterations: u32) -> bool {
        max_outer_loop_iterations > 0 && self.outer_loop_iterations() >= max_outer_loop_iterations
    }

    /// 当前预算使用比例（0–100），取 LLM 次数与 Token 粗估的较高者。
    #[inline]
    pub fn budget_usage_percent(&self, cfg: &TurnBudgetConfig) -> u8 {
        let max_llm = effective_max_llm_calls_per_turn(cfg);
        let llm_pct = if max_llm > 0 {
            ((self.llm_calls() as u64 * 100) / max_llm as u64).min(100) as u8
        } else {
            0
        };
        let token_pct = if cfg.max_turn_tokens > 0 {
            ((self.estimated_tokens() as u64 * 100) / cfg.max_turn_tokens as u64).min(100) as u8
        } else {
            0
        };
        llm_pct.max(token_pct)
    }

    /// 是否已进入预算降级模式（跳过分层非关键验收等）。
    #[inline]
    pub fn is_degradation_active(&self) -> bool {
        self.degradation_active.load(Ordering::Relaxed)
    }

    /// 在记录 LLM 调用或 Token 后检查是否应激活降级（幂等）。
    #[inline]
    pub fn maybe_activate_degradation(&self, cfg: &TurnBudgetConfig) {
        if !cfg.budget_degradation_enabled {
            return;
        }
        let threshold = cfg.budget_degradation_threshold_percent.clamp(50, 99);
        if self.budget_usage_percent(cfg) >= threshold {
            self.degradation_active.store(true, Ordering::Relaxed);
        }
    }

    /// 若已超墙钟、LLM 次数或 Token 上限则返回面向用户的短消息（供 [`complete_chat_retrying`] 等统一门禁）。
    #[inline]
    pub fn deny_llm_call_if_exhausted(&self, cfg: &TurnBudgetConfig) -> Result<(), String> {
        if self.wall_clock_exceeded(cfg) {
            return Err(turn_wall_clock_limit_user_message(
                cfg.max_turn_duration_seconds,
            ));
        }
        let max_llm = effective_max_llm_calls_per_turn(cfg);
        if self.llm_calls_exceeded(max_llm) {
            return Err(turn_llm_calls_limit_user_message(max_llm));
        }
        if self.tokens_exceeded(cfg.max_turn_tokens) {
            return Err(turn_tokens_limit_user_message(cfg.max_turn_tokens));
        }
        Ok(())
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

    #[test]
    fn deny_llm_when_calls_at_cap() {
        let c = TurnBudgetCounter::new_shared();
        let mut cfg = crabmate_config::load_config(None).expect("embed default config");
        cfg.turn_budget.max_llm_calls_per_turn = 2;
        assert!(c.deny_llm_call_if_exhausted(&cfg.turn_budget).is_ok());
        c.record_llm_call();
        c.record_llm_call();
        assert!(c.deny_llm_call_if_exhausted(&cfg.turn_budget).is_err());
    }

    #[test]
    fn deny_llm_when_tokens_at_cap() {
        let c = TurnBudgetCounter::new_shared();
        let mut cfg = crabmate_config::load_config(None).expect("embed default config");
        cfg.turn_budget.max_turn_tokens = 100;
        c.record_estimated_tokens(100);
        assert!(c.deny_llm_call_if_exhausted(&cfg.turn_budget).is_err());
    }

    #[test]
    fn degradation_activates_at_threshold() {
        let c = TurnBudgetCounter::new_shared();
        let mut cfg = crabmate_config::load_config(None).expect("embed default config");
        cfg.turn_budget.budget_degradation_enabled = true;
        cfg.turn_budget.budget_degradation_threshold_percent = 80;
        cfg.turn_budget.max_llm_calls_per_turn = 10;
        assert!(!c.is_degradation_active());
        for _ in 0..8 {
            c.record_llm_call();
        }
        c.maybe_activate_degradation(&cfg.turn_budget);
        assert!(c.is_degradation_active());
    }

    #[test]
    fn is_budget_limit_message_detects_known_phrases() {
        assert!(is_turn_budget_limit_user_message(
            &turn_tokens_limit_user_message(1000)
        ));
        assert!(!is_turn_budget_limit_user_message("other error"));
    }
}
