//! 单轮墙钟、LLM 次数与 Token 粗估预算：**`AgentConfig::turn_budget`** 与 ReAct 外循环 /
//! 侧向 LLM（语义检查等）共享同一判定与面向用户的文案。
//!
//! 会话侧 **messages 裁剪**仍由 **`context_window` / `message_pipeline`** 负责；本模块表达「本轮是否超墙钟 / 超 LLM 次数 / 超 Token 粗估」。
//! 状态归属见 **`docs/design/run_loop_state_ownership.md`**。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::time::Instant;

use crate::cm_config::TurnBudgetConfig;

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

/// 跨回合累计的**起点**（`#1455` B 步）。
///
/// 用途：把上一次暂停时的累计消耗注入新回合的计数器，让 `*_exceeded` 的 `>=` 比较
/// **天然覆盖跨回合口径——判定逻辑一行不改**（见设计文档判据 1）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BudgetSpent {
    pub llm_calls: u32,
    pub estimated_tokens: usize,
    /// 已消耗墙钟秒数。作为**偏移**注入：`Instant` 不可持久化、跨进程无意义，
    /// 所以这里记「累计秒数」而不是「起始时刻」。不含暂停等待时间（暂停期间没消耗）。
    pub elapsed_secs: u64,
}

/// 跨回合累计上限（`#1455` B 步；全 `0` = 不限制）。
///
/// 与 [`TurnBudgetConfig`] 的 `max_turn_*` **刻意分开**：同一配置键不得因另一个开关
/// 改变语义（否则运维无法预测「开了 autonomy 之后单轮上限去哪了」）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CumulativeCaps {
    pub llm_calls: u32,
    pub tokens: usize,
    pub secs: u64,
}

/// 单轮共享预算计数（`Arc` 供分层并行子任务与外循环共用）。
#[derive(Debug)]
pub struct TurnBudgetCounter {
    started_at: Instant,
    llm_calls: AtomicU32,
    outer_loop_iterations: AtomicU32,
    estimated_tokens: AtomicUsize,
    degradation_active: AtomicBool,
    /// 已向模型提示过的预算档位（`0` / `70` / `90`）。**跨档才再提示**：同一档内每轮重发
    /// 会让每轮 `system` 都变，白废一次 prompt cache（#1326 方向的反向操作）。
    notice_tier: std::sync::atomic::AtomicU8,
    /// 注入的完整起点（含墙钟偏移）。计数器从起点开始累加，所以 `llm_calls()` 等读到的
    /// 已是**累计**口径；调用方用它换算回「本回合增量」。
    base: BudgetSpent,
}

impl TurnBudgetCounter {
    #[inline]
    pub fn new_shared() -> Arc<Self> {
        Self::new_shared_with_spent(BudgetSpent::default())
    }

    /// 带**跨回合起点**的计数器（`#1455` B 步）。
    ///
    /// 语义是「累计 = 起点 + 本回合增量」：起点只喂计数器初值，判定逻辑不变。
    #[must_use]
    pub fn new_shared_with_spent(prior: BudgetSpent) -> Arc<Self> {
        Arc::new(Self {
            started_at: Instant::now(),
            llm_calls: AtomicU32::new(prior.llm_calls),
            outer_loop_iterations: AtomicU32::new(0),
            estimated_tokens: AtomicUsize::new(prior.estimated_tokens),
            degradation_active: AtomicBool::new(false),
            notice_tier: std::sync::atomic::AtomicU8::new(0),
            base: prior,
        })
    }

    /// 注入的起点（无历史时全 `0`）。调用方用它把累计值换算回本回合增量。
    #[inline]
    #[must_use]
    pub fn spent_base(&self) -> BudgetSpent {
        self.base
    }

    /// **本回合增量** LLM 调用数（不含注入起点）。
    ///
    /// 单轮上限判定与预算压力展示都必须用它，否则「开了 autonomy 就把单轮上限变成
    /// 累计上限」——那正是配置语义漂移（见 [`CumulativeCaps`] 文档）。
    #[inline]
    #[must_use]
    pub fn llm_calls_this_turn(&self) -> u32 {
        self.llm_calls().saturating_sub(self.base.llm_calls)
    }

    /// **本回合增量** Token 粗估（不含注入起点）。
    #[inline]
    #[must_use]
    pub fn estimated_tokens_this_turn(&self) -> usize {
        self.estimated_tokens()
            .saturating_sub(self.base.estimated_tokens)
    }

    /// **本回合真实耗时**秒（不含注入的墙钟偏移）。
    #[inline]
    #[must_use]
    pub fn elapsed_secs_this_turn(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    #[inline]
    pub fn elapsed_secs(&self) -> u64 {
        self.base
            .elapsed_secs
            .saturating_add(self.started_at.elapsed().as_secs())
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

    /// 墙钟是否已超**单轮**上限（只算本回合真实耗时，不含注入偏移）。
    #[inline]
    pub fn wall_clock_exceeded(&self, cfg: &TurnBudgetConfig) -> bool {
        turn_wall_clock_exceeded(cfg.max_turn_duration_seconds, self.elapsed_secs_this_turn())
    }

    /// **本回合** LLM 调用是否已超单轮上限（`max_llm_calls_per_turn == 0` 表示不限制）。
    #[inline]
    pub fn llm_calls_exceeded(&self, max_llm_calls_per_turn: u32) -> bool {
        max_llm_calls_per_turn > 0 && self.llm_calls_this_turn() >= max_llm_calls_per_turn
    }

    /// **本回合** Token 粗估是否已超单轮上限（`max_turn_tokens == 0` 表示不限制）。
    #[inline]
    pub fn tokens_exceeded(&self, max_turn_tokens: usize) -> bool {
        max_turn_tokens > 0 && self.estimated_tokens_this_turn() >= max_turn_tokens
    }

    /// 是否已超**跨回合累计**上限（`CumulativeCaps` 全 `0` 时恒 `false`）。
    ///
    /// 用累计口径（`llm_calls()` / `estimated_tokens()` / `elapsed_secs()`，均含注入起点），
    /// 与上面的单轮判定严格分开——两组上限互不改写语义。
    #[inline]
    pub fn deny_if_cumulative_exhausted(
        &self,
        caps: &CumulativeCaps,
    ) -> Result<(), TurnBudgetDeny> {
        if caps.secs > 0 && self.elapsed_secs() >= caps.secs {
            return Err(TurnBudgetDeny::WallClock);
        }
        if caps.llm_calls > 0 && self.llm_calls() >= caps.llm_calls {
            return Err(TurnBudgetDeny::LlmCalls);
        }
        if caps.tokens > 0 && self.estimated_tokens() >= caps.tokens {
            return Err(TurnBudgetDeny::Tokens);
        }
        Ok(())
    }

    /// 外循环迭代是否已超上限（`max_outer_loop_iterations == 0` 表示不限制）。
    #[inline]
    pub fn outer_loop_iterations_exceeded(&self, max_outer_loop_iterations: u32) -> bool {
        max_outer_loop_iterations > 0 && self.outer_loop_iterations() >= max_outer_loop_iterations
    }

    /// 当前**本回合**预算使用比例（0–100），取 LLM 次数与 Token 粗估的较高者。
    ///
    /// 用本回合增量而非累计值：压力提示与降级开关的语义是「这一回合快撑不住了」，
    /// 若按累计算，开了 autonomy 的续跑回合会一开局就 90%+，立刻降级 + 发提示。
    #[inline]
    pub fn budget_usage_percent(&self, cfg: &TurnBudgetConfig) -> u8 {
        let max_llm = effective_max_llm_calls_per_turn(cfg);
        let llm_pct = if max_llm > 0 {
            ((self.llm_calls_this_turn() as u64 * 100) / max_llm as u64).min(100) as u8
        } else {
            0
        };
        let token_pct = if cfg.max_turn_tokens > 0 {
            ((self.estimated_tokens_this_turn() as u64 * 100) / cfg.max_turn_tokens as u64).min(100)
                as u8
        } else {
            0
        };
        llm_pct.max(token_pct)
    }

    /// 预算跨档时返回新档位（`70` / `90`），并把档位记为已提示；同档或更低返回 `None`。
    ///
    /// 档位阈值与 [`crate::cm_agent::context_budget_pressure`] 的收紧阈值**同一组**
    /// （70 / 90）：那边收紧裁剪策略、这边告诉模型，两处不能各写一套数字。
    ///
    /// 只提示一次是因为提示行会进 `system`：同一档内每轮重发会让每轮前缀都变，
    /// 白废一次 prompt cache。用 `fetch_max` 而不是 `store`，并发调用下不会把
    /// 已提示的更高档位写低。
    #[inline]
    pub fn claim_budget_notice_tier(&self, cfg: &TurnBudgetConfig) -> Option<u8> {
        let pct = self.budget_usage_percent(cfg);
        let tier = if pct >= 90 {
            90
        } else if pct >= 70 {
            70
        } else {
            return None;
        };
        if self.notice_tier.fetch_max(tier, Ordering::Relaxed) >= tier {
            return None;
        }
        Some(tier)
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

    /// 若已超墙钟、LLM 次数或 Token 上限则返回结构化拒绝原因（供 `complete_chat_retrying` 等统一门禁）；
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `#1455` B 步核心不变量：注入起点后，**单轮**与**累计**两套口径必须能分开读。
    ///
    /// 这是对一次真实缺陷的回归保护：若单轮判定误用累计值（`llm_calls()` 而非
    /// `llm_calls_this_turn()`），「历史 90 + 本回合 5 < 单轮上限 100」会被判超限——
    /// 等于「开 autonomy 就把 `max_turn_*` 静默变成累计上限」。
    #[test]
    fn injected_base_is_not_counted_as_this_turn_usage() {
        let counter = TurnBudgetCounter::new_shared_with_spent(BudgetSpent {
            llm_calls: 90,
            estimated_tokens: 9000,
            elapsed_secs: 500,
        });
        // 累计口径含起点。
        assert_eq!(counter.llm_calls(), 90);
        assert_eq!(counter.estimated_tokens(), 9000);
        assert_eq!(counter.elapsed_secs(), 500);
        // 本回合增量从 0 起——单轮判定与预算压力都读这一组。
        assert_eq!(counter.llm_calls_this_turn(), 0);
        assert_eq!(counter.estimated_tokens_this_turn(), 0);
        assert_eq!(counter.elapsed_secs_this_turn(), 0);

        counter.record_llm_call();
        counter.record_estimated_tokens(100);
        assert_eq!(counter.llm_calls(), 91, "累计含起点");
        assert_eq!(counter.llm_calls_this_turn(), 1, "本回合只用了 1 次");
        assert_eq!(counter.estimated_tokens_this_turn(), 100);
    }

    /// 累计上限全 0 = 不限制（默认口径，保证「开着但不配」与今天行为一致）。
    #[test]
    fn zero_cumulative_caps_mean_unlimited() {
        let counter = TurnBudgetCounter::new_shared_with_spent(BudgetSpent {
            llm_calls: 1_000_000,
            estimated_tokens: 1_000_000,
            elapsed_secs: 1_000_000,
        });
        assert!(
            counter
                .deny_if_cumulative_exhausted(&CumulativeCaps::default())
                .is_ok()
        );
    }

    /// 累计上限按**累计值**触发（与本回合增量无关）。
    #[test]
    fn cumulative_caps_trigger_on_total_not_on_this_turn() {
        let counter = TurnBudgetCounter::new_shared_with_spent(BudgetSpent {
            llm_calls: 90,
            estimated_tokens: 0,
            elapsed_secs: 0,
        });
        let caps = CumulativeCaps {
            llm_calls: 100,
            tokens: 0,
            secs: 0,
        };
        assert!(
            counter.deny_if_cumulative_exhausted(&caps).is_ok(),
            "累计 90 < 100 不该拒"
        );
        for _ in 0..10 {
            counter.record_llm_call();
        }
        assert_eq!(counter.llm_calls_this_turn(), 10, "本回合只用了 10 次");
        assert!(
            matches!(
                counter.deny_if_cumulative_exhausted(&caps),
                Err(TurnBudgetDeny::LlmCalls)
            ),
            "累计达到 100 应拒——与本回合只用了 10 次无关"
        );
    }

    #[test]
    fn zero_means_unlimited_wall_clock() {
        assert!(!turn_wall_clock_exceeded(0, 999_999));
    }

    /// 默认单轮墙钟为 1 小时：真实任务（多轮工具调用 + 构建/测试）常超过 10 分钟。
    ///
    /// 该值可被 `CM_MAX_TURN_DURATION_SECONDS` 覆盖，故仅在未设该环境变量时断言默认值，
    /// 免得开发机或 CI 的覆盖值让测试假失败。
    #[test]
    fn default_wall_clock_is_one_hour() {
        if std::env::var_os("CM_MAX_TURN_DURATION_SECONDS").is_some() {
            return;
        }
        let cfg = crate::cm_config::load_config(None).expect("embed default config");
        assert_eq!(cfg.turn_budget.max_turn_duration_seconds, 3600);
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
