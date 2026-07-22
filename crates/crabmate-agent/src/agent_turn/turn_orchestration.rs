//! 单轮 **`run_agent_turn`** 顶层编排形态（**非**全局 FSM）：供结构化 `tracing` 与排障对齐 `run_dispatch` 分支。
//!
//! 非分层路径由统一 driver（**`non_hierarchical_turn`**）消费 [`NonHierarchicalTurnPhase`]：
//! **`ReAct`**（仅外循环）或 **`PlannedStep`**（无工具规划 + 滚动视界步循环）。

use crabmate_config::{AgentConfig, PlannerExecutorMode};

use crate::agent_turn::staged_planning_gate_types::{
    StagedPlanningDenyReason, StagedPlanningGateOutcome,
};

/// 规划步滚动视界变体（逻辑双代理 vs 单 Agent 规划消息构造）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedStepKind {
    /// `planner_executor_mode == LogicalDualAgent` 且门控放行。
    LogicalDual,
    /// 门控放行时的默认规划步路径（单 Agent `agent_reply_plan`）。
    SingleAgent,
}

impl PlannedStepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LogicalDual => "planned_step_logical_dual",
            Self::SingleAgent => "planned_step_single_agent",
        }
    }
}

/// 非分层回合阶段：外循环（ReAct），或带结构化规划步的滚动视界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonHierarchicalTurnPhase {
    /// 分阶段门控未放行：整轮仅 `run_agent_outer_loop`（ReAct 循环）。
    ReAct,
    /// 门控放行：无工具规划 → 步内外循环 → 步后 replan。
    PlannedStep(PlannedStepKind),
}

impl NonHierarchicalTurnPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReAct => "react",
            Self::PlannedStep(k) => k.as_str(),
        }
    }
}

/// 解析非分层回合阶段（纯函数；**不**再读取 `staged_plan_execution` 作顶层分叉）。
pub fn resolve_non_hierarchical_turn_phase(
    cfg: &AgentConfig,
    staged_intent_gate_allow: bool,
) -> NonHierarchicalTurnPhase {
    if !staged_intent_gate_allow {
        return NonHierarchicalTurnPhase::ReAct;
    }
    if cfg.per_plan_policy.planner_executor_mode == PlannerExecutorMode::SingleAgent {
        return NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::LogicalDual);
    }
    NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::SingleAgent)
}

/// 非分层、**`intent_at_turn_start` 已继续** 时：聚合门控、配置与 [`NonHierarchicalTurnPhase`]。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonHierarchicalTurnResolution {
    pub turn_phase: NonHierarchicalTurnPhase,
    pub orchestration_mode: TurnOrchestrationMode,
    /// 仅当 [`NonHierarchicalTurnPhase::ReAct`] 时有值。
    pub freeform_because: Option<ReActBecause>,
}

/// 非分层下走 **`ReAct`** 的根因（门控拒绝）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReActBecause {
    StagedIntentGateDenied(StagedPlanningDenyReason),
}

impl ReActBecause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StagedIntentGateDenied(r) => r.as_str(),
        }
    }
}

impl NonHierarchicalTurnResolution {
    pub fn resolve(cfg: &AgentConfig, staged_gate: &StagedPlanningGateOutcome) -> Self {
        let allow_staged = staged_gate.allows_staged_planning();
        let turn_phase = resolve_non_hierarchical_turn_phase(cfg, allow_staged);
        let orchestration_mode = TurnOrchestrationMode::from(turn_phase);
        let freeform_because = match turn_phase {
            NonHierarchicalTurnPhase::ReAct => Some(match staged_gate {
                StagedPlanningGateOutcome::Deny { reason, .. } => {
                    ReActBecause::StagedIntentGateDenied(*reason)
                }
                StagedPlanningGateOutcome::Allow { .. } => {
                    unreachable!("ReAct requires staged gate deny")
                }
            }),
            NonHierarchicalTurnPhase::PlannedStep(_) => None,
        };
        Self {
            turn_phase,
            orchestration_mode,
            freeform_because,
        }
    }
}

impl From<NonHierarchicalTurnPhase> for TurnOrchestrationMode {
    fn from(phase: NonHierarchicalTurnPhase) -> Self {
        match phase {
            NonHierarchicalTurnPhase::ReAct => Self::ReAct,
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::LogicalDual) => {
                Self::PlannedStepLogicalDual
            }
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::SingleAgent) => {
                Self::PlannedStepSingleAgent
            }
        }
    }
}

/// 本轮实际进入的主执行形态（在已知分支条件后记录；**不含**分层内 Manager/Operator 子阶段）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOrchestrationMode {
    /// `planner_executor_mode == Hierarchical`，由 `hierarchy::run_hierarchical_agent` 驱动。
    Hierarchical,
    /// 非分层且 `intent_at_turn_start` 已写入终答并结束本回合。
    IntentAtTurnStartFinished,
    /// 非分层、门控放行、`LogicalDualAgent` 规划步滚动视界。
    PlannedStepLogicalDual,
    /// 非分层、门控放行、单 Agent 规划步滚动视界。
    PlannedStepSingleAgent,
    /// 非分层、门控未放行：整轮 `run_agent_outer_loop`（ReAct 循环）。
    ReAct,
}

impl TurnOrchestrationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hierarchical => "hierarchical",
            Self::IntentAtTurnStartFinished => "intent_at_turn_start_finished",
            Self::PlannedStepLogicalDual => "planned_step_logical_dual",
            Self::PlannedStepSingleAgent => "planned_step_single_agent",
            Self::ReAct => "react",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(mode: PlannerExecutorMode) -> AgentConfig {
        let mut c = crabmate_config::load_config(None).expect("embed default config");
        c.per_plan_policy.planner_executor_mode = mode;
        c
    }

    #[test]
    fn logical_dual_planned_step_when_gate_allows() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        assert_eq!(
            resolve_non_hierarchical_turn_phase(&cfg, true),
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::LogicalDual)
        );
        assert_eq!(
            TurnOrchestrationMode::from(resolve_non_hierarchical_turn_phase(&cfg, true)),
            TurnOrchestrationMode::PlannedStepLogicalDual
        );
    }

    #[test]
    fn single_agent_planned_step_when_gate_allows() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        assert_eq!(
            resolve_non_hierarchical_turn_phase(&cfg, true),
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::SingleAgent)
        );
    }

    #[test]
    fn freeform_when_gate_denies() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        assert_eq!(
            resolve_non_hierarchical_turn_phase(&cfg, false),
            NonHierarchicalTurnPhase::ReAct
        );
    }

    #[test]
    fn gate_allow_always_planned_step_when_intent_gate_allows() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        assert_eq!(
            resolve_non_hierarchical_turn_phase(&cfg, true),
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::SingleAgent)
        );
    }

    #[test]
    fn turn_resolution_denied_gate_carries_reason() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let gate = StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
        let r = NonHierarchicalTurnResolution::resolve(&cfg, &gate);
        assert_eq!(r.turn_phase, NonHierarchicalTurnPhase::ReAct);
        assert_eq!(
            r.freeform_because,
            Some(ReActBecause::StagedIntentGateDenied(
                StagedPlanningDenyReason::EmptyEffectiveTask
            ))
        );
    }

    #[test]
    fn turn_resolution_allow_yields_planned_step_without_freeform_because() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        use crate::intent_pipeline::{IntentAction, IntentDecision};
        use crate::intent_router::IntentKind;
        let gate = StagedPlanningGateOutcome::Allow {
            task_preview: "t".into(),
            intent_kind: IntentKind::Execute,
            primary_intent: "execute.test".into(),
            confidence: 0.9,
            decision: IntentDecision {
                kind: IntentKind::Execute,
                primary_intent: "execute.test".into(),
                secondary_intents: Vec::new(),
                confidence: 0.9,
                abstain: false,
                need_clarification: false,
                action: IntentAction::Execute,
                multi_intent: None,
            },
        };
        let r = NonHierarchicalTurnResolution::resolve(&cfg, &gate);
        assert_eq!(
            r.turn_phase,
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::SingleAgent)
        );
        assert!(r.freeform_because.is_none());
    }

    #[test]
    fn turn_resolution_logical_dual_no_freeform_because() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        use crate::intent_pipeline::{IntentAction, IntentDecision};
        use crate::intent_router::IntentKind;
        let gate = StagedPlanningGateOutcome::Allow {
            task_preview: "t".into(),
            intent_kind: IntentKind::Execute,
            primary_intent: "execute.test".into(),
            confidence: 0.9,
            decision: IntentDecision {
                kind: IntentKind::Execute,
                primary_intent: "execute.test".into(),
                secondary_intents: Vec::new(),
                confidence: 0.9,
                abstain: false,
                need_clarification: false,
                action: IntentAction::Execute,
                multi_intent: None,
            },
        };
        let r = NonHierarchicalTurnResolution::resolve(&cfg, &gate);
        assert_eq!(
            r.turn_phase,
            NonHierarchicalTurnPhase::PlannedStep(PlannedStepKind::LogicalDual)
        );
        assert!(r.freeform_because.is_none());
    }

    #[test]
    fn turn_resolution_advisory_bypass_yields_freeform() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        use crate::intent_pipeline::{IntentAction, IntentDecision};
        use crate::intent_router::IntentKind;
        let gate = StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::AdvisoryExecuteBypassStaged,
            task_preview: Some("t".into()),
            intent_decision: Some(IntentDecision {
                kind: IntentKind::Execute,
                primary_intent: "execute.code_change".into(),
                secondary_intents: Vec::new(),
                confidence: 0.9,
                abstain: false,
                need_clarification: false,
                action: IntentAction::Execute,
                multi_intent: None,
            }),
        };
        let r = NonHierarchicalTurnResolution::resolve(&cfg, &gate);
        assert_eq!(r.turn_phase, NonHierarchicalTurnPhase::ReAct);
        assert_eq!(
            r.freeform_because,
            Some(ReActBecause::StagedIntentGateDenied(
                StagedPlanningDenyReason::AdvisoryExecuteBypassStaged
            ))
        );
    }
}
