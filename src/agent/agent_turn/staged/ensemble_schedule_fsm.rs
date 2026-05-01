//! 逻辑多规划员（ensemble）**驱动阶段**：辅助规划员串行次数与合并轮是否值得发起。
//! **纯函数**，与 **`ensemble_fsm`**（单轮解析/合并决策）互补；**不**发起 LLM。
//!
//! 调用点：**`maybe_run_staged_plan_ensemble_then_merge`**（`staged/mod.rs`）。

/// **`maybe_run_staged_plan_ensemble_then_merge`** 顶层驱动阶段（隐式早退 → 显式枚举）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EnsembleDriverPhase {
    /// **`extra == 0`**（`staged_plan_ensemble_count <= 1`）或寒暄跳过：**不**注入 coach user、**不**合并。
    Done,
    /// 注入 **`extra`** 次辅助规划员 user，按 **`ensemble_secondary_planner_display_index`** 编号。
    SecondaryChain { extra: u8 },
}

/// 与 `extra = count - 1` 及早退分支等价。
#[inline]
pub(crate) fn resolve_ensemble_driver_phase(
    staged_plan_ensemble_count: u8,
    skip_for_casual_user_prompt: bool,
) -> EnsembleDriverPhase {
    let extra = staged_plan_ensemble_count.saturating_sub(1);
    if extra == 0 || skip_for_casual_user_prompt {
        EnsembleDriverPhase::Done
    } else {
        EnsembleDriverPhase::SecondaryChain { extra }
    }
}

/// 第 **`i`** 个辅助规划员轮（**`0..extra`**，`u8`）对应的展示编号（与既有日志 **`planner_idx = i + 2`** 一致）。
#[inline]
pub(crate) fn ensemble_secondary_planner_display_index(chain_round_index: u8) -> u8 {
    chain_round_index.saturating_add(2)
}

/// 是否进入合并轮：**至少两份**采纳草案（首轮克隆 + ≥1 份辅助）。
#[inline]
pub(crate) fn ensemble_merge_should_run(accepted_plans_len: usize) -> bool {
    accepted_plans_len >= 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn done_when_count_le_one() {
        assert_eq!(
            resolve_ensemble_driver_phase(1, false),
            EnsembleDriverPhase::Done
        );
    }

    #[test]
    fn done_when_casual_skip_even_if_multi() {
        assert_eq!(
            resolve_ensemble_driver_phase(3, true),
            EnsembleDriverPhase::Done
        );
    }

    #[test]
    fn chain_when_extra_positive_and_not_skipped() {
        match resolve_ensemble_driver_phase(3, false) {
            EnsembleDriverPhase::SecondaryChain { extra } => assert_eq!(extra, 2),
            EnsembleDriverPhase::Done => panic!("expected SecondaryChain"),
        }
    }

    #[test]
    fn display_index_matches_legacy_plus_two() {
        assert_eq!(ensemble_secondary_planner_display_index(0), 2);
        assert_eq!(ensemble_secondary_planner_display_index(1), 3);
    }

    #[test]
    fn merge_only_with_two_or_more_plans() {
        assert!(!ensemble_merge_should_run(1));
        assert!(ensemble_merge_should_run(2));
    }
}
