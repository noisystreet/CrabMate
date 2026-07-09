//! 回合顶层编排：**单一决策入口**与结构化阶段迁移（与 `tracing` 字段 `orchestration_transition` 对齐）。
//!
//! 非分层主路径解析见 [`super::turn_orchestration`]；分层意图后路由见 [`super::hierarchical_intent_route`]。

use crabmate_config::{AgentConfig, PlannerExecutorMode};

use crate::intent_pipeline::IntentDecision;

use super::hierarchical_intent_route::{
    HierarchicalPostIntentRoute, resolve_hierarchical_post_intent_route,
};
use super::staged_planning_gate_types::StagedPlanningGateOutcome;
use super::turn_orchestration::{NonHierarchicalTurnResolution, TurnOrchestrationMode};

/// `run_agent_turn_common` 顶层二分：分层 vs 非分层（仅读配置，无 IO）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnTopLevelDispatch {
    Hierarchical,
    NonHierarchical,
}

impl TurnTopLevelDispatch {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hierarchical => "hierarchical",
            Self::NonHierarchical => "non_hierarchical",
        }
    }
}

/// 结构化阶段迁移标签（顶层；不含外循环 P/R/E 细粒度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnOrchestrationTransition {
    EnterCommon,
    DispatchHierarchical,
    DispatchNonHierarchical,
    HierarchicalPostIntentResolved,
    NonHierarchicalEntryResolved,
}

impl TurnOrchestrationTransition {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EnterCommon => "enter_common",
            Self::DispatchHierarchical => "dispatch_hierarchical",
            Self::DispatchNonHierarchical => "dispatch_non_hierarchical",
            Self::HierarchicalPostIntentResolved => "hierarchical_post_intent_resolved",
            Self::NonHierarchicalEntryResolved => "non_hierarchical_entry_resolved",
        }
    }
}

/// 由 [`AgentConfig::per_plan_policy.planner_executor_mode`] 解析顶层分发。
#[inline]
pub fn resolve_turn_top_level_dispatch(cfg: &AgentConfig) -> TurnTopLevelDispatch {
    if cfg.per_plan_policy.planner_executor_mode == PlannerExecutorMode::Hierarchical {
        TurnTopLevelDispatch::Hierarchical
    } else {
        TurnTopLevelDispatch::NonHierarchical
    }
}

/// 分层路径：`ProceedExecute` 之后的下一执行面与对外 `turn_orchestration_mode`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchicalTurnEntryResolution {
    pub post_intent_route: HierarchicalPostIntentRoute,
    pub orchestration_mode: TurnOrchestrationMode,
}

impl HierarchicalTurnEntryResolution {
    pub fn resolve(assessment: &IntentDecision) -> Self {
        let post_intent_route = resolve_hierarchical_post_intent_route(assessment);
        let orchestration_mode = match post_intent_route {
            HierarchicalPostIntentRoute::DiscourseFallbackOuter(_) => {
                TurnOrchestrationMode::Freeform
            }
            HierarchicalPostIntentRoute::RouterManagerRunner => TurnOrchestrationMode::Hierarchical,
        };
        Self {
            post_intent_route,
            orchestration_mode,
        }
    }
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
    use crate::agent_turn::turn_orchestration::NonHierarchicalTurnPhase;
    use crate::intent_pipeline::{IntentAction, IntentDecision};
    use crate::intent_router::IntentKind;

    fn cfg_with(mode: PlannerExecutorMode) -> AgentConfig {
        let mut c = crabmate_config::load_config(None).expect("embed default config");
        c.per_plan_policy.planner_executor_mode = mode;
        c
    }

    fn decision(kind: IntentKind, primary: &str, action: IntentAction) -> IntentDecision {
        IntentDecision {
            kind,
            primary_intent: primary.to_string(),
            secondary_intents: Vec::new(),
            confidence: 0.9,
            abstain: false,
            need_clarification: false,
            action,
            multi_intent: None,
        }
    }

    #[test]
    fn top_level_dispatch_from_config() {
        assert_eq!(
            resolve_turn_top_level_dispatch(&cfg_with(PlannerExecutorMode::Hierarchical)),
            TurnTopLevelDispatch::Hierarchical
        );
        assert_eq!(
            resolve_turn_top_level_dispatch(&cfg_with(PlannerExecutorMode::SingleAgent)),
            TurnTopLevelDispatch::NonHierarchical
        );
    }

    #[test]
    fn hierarchical_entry_maps_discourse_to_outer_loop_mode() {
        let d = decision(
            IntentKind::Greeting,
            "meta.greeting",
            IntentAction::DirectReply("".into()),
        );
        let r = HierarchicalTurnEntryResolution::resolve(&d);
        assert!(matches!(
            r.post_intent_route,
            HierarchicalPostIntentRoute::DiscourseFallbackOuter(_)
        ));
        assert_eq!(r.orchestration_mode, TurnOrchestrationMode::Freeform);
    }

    #[test]
    fn hierarchical_entry_execute_stays_hierarchical() {
        let d = decision(
            IntentKind::Execute,
            "execute.read_inspect",
            IntentAction::Execute,
        );
        let r = HierarchicalTurnEntryResolution::resolve(&d);
        assert_eq!(
            r.post_intent_route,
            HierarchicalPostIntentRoute::RouterManagerRunner
        );
        assert_eq!(r.orchestration_mode, TurnOrchestrationMode::Hierarchical);
    }

    #[test]
    fn non_hierarchical_turn_delegates_to_turn_orchestration() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent);
        let gate = StagedPlanningGateOutcome::Deny {
            reason: super::super::staged_planning_gate_types::StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
        let r = resolve_non_hierarchical_turn(&cfg, &gate);
        assert_eq!(r.turn_phase, NonHierarchicalTurnPhase::Freeform);
    }
}
