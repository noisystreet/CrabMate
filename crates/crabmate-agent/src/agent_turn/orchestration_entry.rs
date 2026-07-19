//! 回合顶层编排：**单一决策入口**与结构化阶段迁移（与 `tracing` 字段 `orchestration_transition` 对齐）。

use crabmate_config::AgentConfig;

use super::staged_planning_gate_types::StagedPlanningGateOutcome;
use super::turn_orchestration::NonHierarchicalTurnResolution;

/// `run_agent_turn_common` 顶层分发：当前仅非分层模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnTopLevelDispatch {
    NonHierarchical,
}

impl TurnTopLevelDispatch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NonHierarchical => "non_hierarchical",
        }
    }
}

/// 结构化阶段迁移标签（顶层；不含外循环 P/R/E 细粒度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOrchestrationTransition {
    EnterCommon,
    DispatchNonHierarchical,
    NonHierarchicalEntryResolved,
}

impl TurnOrchestrationTransition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnterCommon => "enter_common",
            Self::DispatchNonHierarchical => "dispatch_non_hierarchical",
            Self::NonHierarchicalEntryResolved => "non_hierarchical_entry_resolved",
        }
    }
}

/// 由 [`AgentConfig::per_plan_policy.planner_executor_mode`] 解析顶层分发。
#[inline]
pub fn resolve_turn_top_level_dispatch(_cfg: &AgentConfig) -> TurnTopLevelDispatch {
    TurnTopLevelDispatch::NonHierarchical
}

/// 非分层：`staged_plan_intent_gate` 结果 + 配置 → 回合阶段（与 [`NonHierarchicalTurnResolution::resolve`] 对齐）。
#[inline]
pub fn resolve_non_hierarchical_turn(
    cfg: &AgentConfig,
    staged_gate: &StagedPlanningGateOutcome,
) -> NonHierarchicalTurnResolution {
    NonHierarchicalTurnResolution::resolve(cfg, staged_gate)
}

/// 统一 info 日志字段，减少 `mod.rs` / `run_dispatch` / `hierarchy` 散落叙述。
pub fn log_orchestration_transition(
    transition: TurnOrchestrationTransition,
    turn_orchestration_mode: Option<&str>,
    extra: &[(&str, &str)],
) {
    let mode = turn_orchestration_mode.unwrap_or("");
    if extra.is_empty() {
        log::info!(
            target: "crabmate::agent_turn",
            "orchestration transition transition={} turn_orchestration_mode={}",
            transition.as_str(),
            mode
        );
    } else {
        let extras: String = extra
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(" ");
        log::info!(
            target: "crabmate::agent_turn",
            "orchestration transition transition={} turn_orchestration_mode={} {extras}",
            transition.as_str(),
            mode
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_turn::staged_planning_gate_types::StagedPlanningDenyReason;
    use crate::agent_turn::turn_orchestration::NonHierarchicalTurnPhase;
    use crabmate_config::PlannerExecutorMode;

    fn cfg_with(mode: PlannerExecutorMode) -> AgentConfig {
        let mut c = crabmate_config::load_config(None).expect("embed default config");
        c.per_plan_policy.planner_executor_mode = mode;
        c
    }

    #[test]
    fn top_level_dispatch_always_non_hierarchical() {
        assert_eq!(
            resolve_turn_top_level_dispatch(&cfg_with(PlannerExecutorMode::Hierarchical)),
            TurnTopLevelDispatch::NonHierarchical
        );
        assert_eq!(
            resolve_turn_top_level_dispatch(&cfg_with(PlannerExecutorMode::SingleAgent)),
            TurnTopLevelDispatch::NonHierarchical
        );
    }

    #[test]
    fn non_hierarchical_turn_delegates_to_turn_orchestration() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let gate = StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
        let r = resolve_non_hierarchical_turn(&cfg, &gate);
        assert_eq!(r.turn_phase, NonHierarchicalTurnPhase::ReAct);
    }
}
