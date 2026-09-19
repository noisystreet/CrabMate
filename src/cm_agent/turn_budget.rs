//! 单轮墙钟、LLM 次数与 Token 粗估预算：**`AgentConfig::turn_budget`** 与 ReAct 外循环 /
//! 侧向 LLM（语义检查等）共享同一判定与面向用户的文案。
//!
//! 会话侧 **messages 裁剪**仍由 **`context_window` / `message_pipeline`** 负责；本模块表达「本轮是否超墙钟 / 超 LLM 次数 / 超 Token 粗估」。
//! 状态归属见 **`docs/design/run_loop_state_ownership.md`**。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

use crate::cm_config::TurnBudgetConfig;

/// LLM 调用的**观测分类**（扩展点；**不**改变现有 deny / 计数逻辑）。
///
/// 现行 [`TurnBudgetCounter`] 为整轮共享计数；未来若拆 planner / executor / 侧向子预算，
/// 可用本枚举作 tracing 标签。侧向（`final_plan_semantic_check`、未来 audience）计入共享墙钟。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmCallBudgetClass {
    /// 外循环第 1 轮（及明确 planner 端点）模型调用。
    Planner,
    /// 外循环 ≥2 轮 executor 端点模型调用。
    Executor,
    /// 无工具侧向检查（语义一致性 / 预留观众角色）。
    SideCheck,
}

impl LlmCallBudgetClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Executor => "executor",
            Self::SideCheck => "side_check",
        }
    }
}

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

/// [`TurnBudgetCounter::deny_llm_call_if_exhausted`] 的结构化拒绝原因。
///
/// 调用方应据此分支（如墙钟 → `RunAgentTurnError::TimeLimitExhausted`），
/// 而**不要**再嗅探 [`Self::user_message`] 的文案子串。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnBudgetDeny {
    /// 超单轮墙钟上限。
    WallClock,
    /// 超单轮 LLM 调用次数上限。
    LlmCalls,
    /// 超单轮 Token 粗估上限。
    Tokens,
}

impl TurnBudgetDeny {
    /// 稳定标识（用于 tracing / 日志，不面向用户）。
    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WallClock => "wall_clock",
            Self::LlmCalls => "llm_calls",
            Self::Tokens => "tokens",
        }
    }

    /// 面向用户的短消息（与既有 SSE 文案逐字一致）。
    #[inline]
    pub fn user_message(self, cfg: &TurnBudgetConfig) -> String {
        match self {
            Self::WallClock => turn_wall_clock_limit_user_message(cfg.max_turn_duration_seconds),
            Self::LlmCalls => {
                turn_llm_calls_limit_user_message(effective_max_llm_calls_per_turn(cfg))
            }
            Self::Tokens => turn_tokens_limit_user_message(cfg.max_turn_tokens),
        }
    }
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

    /// 若已超墙钟、LLM 次数或 Token 上限则返回结构化拒绝原因（供 [`complete_chat_retrying`] 等统一门禁）；
    /// 用户文案由 [`TurnBudgetDeny::user_message`] 生成。
    #[inline]
    pub fn deny_llm_call_if_exhausted(&self, cfg: &TurnBudgetConfig) -> Result<(), TurnBudgetDeny> {
        if self.wall_clock_exceeded(cfg) {
            return Err(TurnBudgetDeny::WallClock);
        }
        let max_llm = effective_max_llm_calls_per_turn(cfg);
        if self.llm_calls_exceeded(max_llm) {
            return Err(TurnBudgetDeny::LlmCalls);
        }
        if self.tokens_exceeded(cfg.max_turn_tokens) {
            return Err(TurnBudgetDeny::Tokens);
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
        let mut cfg = crate::cm_config::load_config(None).expect("embed default config");
        cfg.turn_budget.max_llm_calls_per_turn = 2;
        assert!(c.deny_llm_call_if_exhausted(&cfg.turn_budget).is_ok());
        c.record_llm_call();
        c.record_llm_call();
        assert_eq!(
            c.deny_llm_call_if_exhausted(&cfg.turn_budget),
            Err(TurnBudgetDeny::LlmCalls)
        );
    }

    #[test]
    fn deny_llm_when_tokens_at_cap() {
        let c = TurnBudgetCounter::new_shared();
        let mut cfg = crate::cm_config::load_config(None).expect("embed default config");
        cfg.turn_budget.max_turn_tokens = 100;
        c.record_estimated_tokens(100);
        assert_eq!(
            c.deny_llm_call_if_exhausted(&cfg.turn_budget),
            Err(TurnBudgetDeny::Tokens)
        );
    }

    #[test]
    fn degradation_activates_at_threshold() {
        let c = TurnBudgetCounter::new_shared();
        let mut cfg = crate::cm_config::load_config(None).expect("embed default config");
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
    fn deny_variants_have_stable_labels_and_user_messages() {
        let mut cfg = crate::cm_config::load_config(None).expect("embed default config");
        cfg.turn_budget.max_llm_calls_per_turn = 7;
        cfg.turn_budget.max_turn_tokens = 4096;
        assert_eq!(TurnBudgetDeny::WallClock.as_str(), "wall_clock");
        assert_eq!(TurnBudgetDeny::LlmCalls.as_str(), "llm_calls");
        assert_eq!(TurnBudgetDeny::Tokens.as_str(), "tokens");
        assert_eq!(
            TurnBudgetDeny::WallClock.user_message(&cfg.turn_budget),
            turn_wall_clock_limit_user_message(cfg.turn_budget.max_turn_duration_seconds)
        );
        assert_eq!(
            TurnBudgetDeny::LlmCalls.user_message(&cfg.turn_budget),
            turn_llm_calls_limit_user_message(7)
        );
        assert_eq!(
            TurnBudgetDeny::Tokens.user_message(&cfg.turn_budget),
            turn_tokens_limit_user_message(4096)
        );
    }
}
