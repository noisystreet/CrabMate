//! 单轮 **`run_agent_turn`** 顶层编排形态（**非**全局 FSM）：供结构化 `tracing` 与排障对齐 `run_dispatch` 分支。
//!
//! 见 `docs/开发文档.md`「P/R/E」与 **`run_dispatch`** 说明。

use crate::config::{AgentConfig, PlannerExecutorMode};

/// 本轮实际进入的主执行形态（在已知分支条件后记录；**不含**分层内 Manager/Operator 子阶段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnOrchestrationMode {
    /// `planner_executor_mode == Hierarchical`，由 `hierarchy::run_hierarchical_agent` 驱动。
    Hierarchical,
    /// 非分层且 `intent_at_turn_start` 已写入终答并结束本回合。
    IntentAtTurnStartFinished,
    /// 非分层、`staged_plan_intent_gate` 放行且 `planner_executor_mode == LogicalDualAgent`。
    LogicalDualAgentStaged,
    /// 非分层、意图门控放行且 `staged_plan_execution`。
    StagedPlanExecution,
    /// 非分层默认：`run_agent_outer_loop`（单 Agent P→R→E）。
    SingleAgentOuterLoop,
}

impl TurnOrchestrationMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Hierarchical => "hierarchical",
            Self::IntentAtTurnStartFinished => "intent_at_turn_start_finished",
            Self::LogicalDualAgentStaged => "logical_dual_agent_staged",
            Self::StagedPlanExecution => "staged_plan_execution",
            Self::SingleAgentOuterLoop => "single_agent_outer_loop",
        }
    }
}

/// 在 **`intent_at_turn_start` 已通过**（继续主路径）且已知 **`staged_plan_intent_gate`** 是否放行时，解析非分层主路径。
pub(crate) fn resolve_non_hierarchical_main_path(
    cfg: &AgentConfig,
    staged_intent_gate_allow: bool,
) -> TurnOrchestrationMode {
    if cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent
        && staged_intent_gate_allow
    {
        TurnOrchestrationMode::LogicalDualAgentStaged
    } else if cfg.staged_plan_execution && staged_intent_gate_allow {
        TurnOrchestrationMode::StagedPlanExecution
    } else {
        TurnOrchestrationMode::SingleAgentOuterLoop
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(mode: PlannerExecutorMode, staged_plan_execution: bool) -> AgentConfig {
        let mut c = crate::config::load_config(None).expect("embed default config");
        c.planner_executor_mode = mode;
        c.staged_plan_execution = staged_plan_execution;
        c
    }

    #[test]
    fn logical_dual_wins_when_gate_allows() {
        let cfg = cfg_with(PlannerExecutorMode::LogicalDualAgent, true);
        assert_eq!(
            resolve_non_hierarchical_main_path(&cfg, true),
            TurnOrchestrationMode::LogicalDualAgentStaged
        );
    }

    #[test]
    fn staged_when_dual_disabled_but_staged_on() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent, true);
        assert_eq!(
            resolve_non_hierarchical_main_path(&cfg, true),
            TurnOrchestrationMode::StagedPlanExecution
        );
    }

    #[test]
    fn single_outer_when_gate_denies() {
        let cfg = cfg_with(PlannerExecutorMode::LogicalDualAgent, true);
        assert_eq!(
            resolve_non_hierarchical_main_path(&cfg, false),
            TurnOrchestrationMode::SingleAgentOuterLoop
        );
    }

    #[test]
    fn single_outer_when_staged_off_even_if_gate_allows() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent, false);
        assert_eq!(
            resolve_non_hierarchical_main_path(&cfg, true),
            TurnOrchestrationMode::SingleAgentOuterLoop
        );
    }
}
