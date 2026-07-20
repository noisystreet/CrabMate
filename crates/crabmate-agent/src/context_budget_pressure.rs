//! 单轮 [`TurnBudgetCounter`] 与 [`crate::message_pipeline`] / 上下文摘要的联动策略。
//!
//! 主 Agent 外循环与 Operator ReAct 共用本模块，避免「预算将尽仍按默认 char 预算裁剪」的分叉。

use crabmate_config::AgentConfig;

use crate::turn_budget::TurnBudgetCounter;

/// 预算压力下的上下文裁剪/摘要调节（百分制，`100` 表示不调节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextBudgetPressure {
    pub char_budget_scale_percent: u8,
    pub summary_trigger_cap: Option<usize>,
}

impl Default for ContextBudgetPressure {
    fn default() -> Self {
        Self {
            char_budget_scale_percent: 100,
            summary_trigger_cap: None,
        }
    }
}

/// 根据当前单轮预算用量解析会话同步管道与 LLM 摘要的收紧程度。
#[must_use]
pub fn resolve_context_budget_pressure(
    cfg: &AgentConfig,
    turn_budget: Option<&TurnBudgetCounter>,
) -> ContextBudgetPressure {
    let Some(budget) = turn_budget else {
        return ContextBudgetPressure::default();
    };
    let pct = budget.budget_usage_percent(&cfg.turn_budget);
    if pct >= 90 {
        ContextBudgetPressure {
            char_budget_scale_percent: 50,
            summary_trigger_cap: Some(6_144),
        }
    } else if pct >= 70 {
        ContextBudgetPressure {
            char_budget_scale_percent: 70,
            summary_trigger_cap: Some(12_288),
        }
    } else {
        ContextBudgetPressure::default()
    }
}

/// 在配置基线摘要触发阈值上，按预算压力施加更早触发的上限（`0` 基线仍表示关闭）。
#[must_use]
pub fn effective_summary_trigger_for_turn(
    cfg: &AgentConfig,
    turn_budget: Option<&TurnBudgetCounter>,
) -> usize {
    let base = cfg.effective_context_summary_trigger_chars();
    if base == 0 {
        return 0;
    }
    let pressure = resolve_context_budget_pressure(cfg, turn_budget);
    pressure
        .summary_trigger_cap
        .map(|cap| base.min(cap))
        .unwrap_or(base)
}

/// 将会话同步管道的近似字符预算按压力比例缩放（不低于 4096，除非原值为 0）。
#[must_use]
pub fn scale_message_pipeline_char_budget(base: usize, scale_percent: u8) -> usize {
    if base == 0 || scale_percent >= 100 {
        return base;
    }
    base.saturating_mul(scale_percent as usize)
        .saturating_div(100)
        .max(4_096)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn_budget::TurnBudgetCounter;

    #[test]
    fn no_pressure_without_turn_budget() {
        let cfg = crabmate_config::load_config(None).expect("embed default");
        assert_eq!(
            resolve_context_budget_pressure(&cfg, None),
            ContextBudgetPressure::default()
        );
    }

    #[test]
    fn pressure_at_eighty_percent_token_usage() {
        let cfg = crabmate_config::load_config(None).expect("embed default");
        let mut cfg = cfg;
        cfg.turn_budget.max_turn_tokens = 100;
        let budget = TurnBudgetCounter::new_shared();
        budget.record_estimated_tokens(80);
        let p = resolve_context_budget_pressure(&cfg, Some(budget.as_ref()));
        assert_eq!(p.char_budget_scale_percent, 70);
        assert_eq!(p.summary_trigger_cap, Some(12_288));
    }

    #[test]
    fn pressure_at_ninety_percent_token_usage() {
        let cfg = crabmate_config::load_config(None).expect("embed default");
        let mut cfg = cfg;
        cfg.turn_budget.max_turn_tokens = 100;
        let budget = TurnBudgetCounter::new_shared();
        budget.record_estimated_tokens(95);
        let p = resolve_context_budget_pressure(&cfg, Some(budget.as_ref()));
        assert_eq!(p.char_budget_scale_percent, 50);
        assert_eq!(p.summary_trigger_cap, Some(6_144));
    }

    #[test]
    fn summary_trigger_cap_applied_when_base_larger() {
        let mut cfg = crabmate_config::load_config(None).expect("embed default");
        cfg.context_pipeline.context_summary_trigger_chars = 50_000;
        cfg.turn_budget.max_turn_tokens = 100;
        let budget = TurnBudgetCounter::new_shared();
        budget.record_estimated_tokens(80);
        let trigger = effective_summary_trigger_for_turn(&cfg, Some(budget.as_ref()));
        assert_eq!(trigger, 12_288);
    }

    #[test]
    fn scale_char_budget_respects_floor() {
        assert_eq!(scale_message_pipeline_char_budget(10_000, 50), 5_000);
        assert_eq!(scale_message_pipeline_char_budget(5_000, 50), 4_096);
        assert_eq!(scale_message_pipeline_char_budget(0, 50), 0);
    }
}
