//! 单轮 **`run_agent_turn`** 顶层编排形态（**非**全局 FSM）：供结构化 `tracing` 与排障对齐 `run_dispatch` 分支。
//!
//! 见 `docs/开发文档.md`「P/R/E」与 **`run_dispatch`** 说明。

use crate::config::{AgentConfig, PlannerExecutorMode};

use super::intent::StagedPlanningGateOutcome;
use super::intent::staged_planning_gate::StagedPlanningDenyReason;

/// 非分层、且 **`intent_at_turn_start` 已通过** 且已知 **`staged_plan_intent_gate`** 是否放行时，
/// 主执行路径的**显式枚举**（与 `run_dispatch::dispatch_non_hierarchical_turn` 的 `if` 链一一对应）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NonHierarchicalMainRoute {
    /// `planner_executor_mode == LogicalDualAgent` 且门控放行。
    LogicalDualAgentStaged,
    /// `staged_plan_execution` 且门控放行。
    StagedPlanExecution,
    /// 默认：`run_agent_outer_loop`。
    SingleAgentOuterLoop,
}

impl NonHierarchicalMainRoute {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::LogicalDualAgentStaged => "logical_dual_agent_staged",
            Self::StagedPlanExecution => "staged_plan_execution",
            Self::SingleAgentOuterLoop => "single_agent_outer_loop",
        }
    }
}

/// 解析非分层主路径（纯函数）。
pub(crate) fn resolve_non_hierarchical_main_route(
    cfg: &AgentConfig,
    staged_intent_gate_allow: bool,
) -> NonHierarchicalMainRoute {
    if cfg.planner_executor_mode == PlannerExecutorMode::LogicalDualAgent
        && staged_intent_gate_allow
    {
        NonHierarchicalMainRoute::LogicalDualAgentStaged
    } else if cfg.staged_plan_execution && staged_intent_gate_allow {
        NonHierarchicalMainRoute::StagedPlanExecution
    } else {
        NonHierarchicalMainRoute::SingleAgentOuterLoop
    }
}

/// 非分层、**`intent_at_turn_start` 已继续** 时：聚合 **`staged_plan_intent_gate`**、配置与
/// [`NonHierarchicalMainRoute`]，并给出「若落单 Agent 外循环则为何」的显式原因（供 `tracing` / 回放）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NonHierarchicalEntryResolution {
    pub(crate) main_route: NonHierarchicalMainRoute,
    pub(crate) orchestration_mode: TurnOrchestrationMode,
    /// 仅当 [`NonHierarchicalMainRoute::SingleAgentOuterLoop`] 时有值。
    pub(crate) single_agent_outer_loop_because: Option<SingleAgentOuterLoopBecause>,
}

/// 非分层下最终走 **`run_agent_outer_loop`** 的根因（门控拒绝 vs 配置未命中更高优先级路径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SingleAgentOuterLoopBecause {
    /// [`StagedPlanningGateOutcome::Deny`]：分阶段/逻辑双代理门控未放行。
    StagedIntentGateDenied(StagedPlanningDenyReason),
    /// 门控已 **`Allow`**，但当前 `planner_executor_mode` / `staged_plan_execution` 组合未命中逻辑双代理或分阶段主路径，落在默认单 Agent 外循环。
    ConfigFallbackSingleAgentOuterLoop,
}

impl SingleAgentOuterLoopBecause {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::StagedIntentGateDenied(r) => r.as_str(),
            Self::ConfigFallbackSingleAgentOuterLoop => "config_fallback_single_agent_outer_loop",
        }
    }
}

impl NonHierarchicalEntryResolution {
    pub(crate) fn resolve(cfg: &AgentConfig, staged_gate: &StagedPlanningGateOutcome) -> Self {
        let allow_staged = staged_gate.allows_staged_planning();
        let main_route = resolve_non_hierarchical_main_route(cfg, allow_staged);
        let orchestration_mode = TurnOrchestrationMode::from(main_route);
        let single_agent_outer_loop_because = match main_route {
            NonHierarchicalMainRoute::SingleAgentOuterLoop => Some(match staged_gate {
                StagedPlanningGateOutcome::Deny { reason, .. } => {
                    SingleAgentOuterLoopBecause::StagedIntentGateDenied(*reason)
                }
                StagedPlanningGateOutcome::Allow { .. } => {
                    SingleAgentOuterLoopBecause::ConfigFallbackSingleAgentOuterLoop
                }
            }),
            _ => None,
        };
        Self {
            main_route,
            orchestration_mode,
            single_agent_outer_loop_because,
        }
    }
}

impl From<NonHierarchicalMainRoute> for TurnOrchestrationMode {
    fn from(r: NonHierarchicalMainRoute) -> Self {
        match r {
            NonHierarchicalMainRoute::LogicalDualAgentStaged => Self::LogicalDualAgentStaged,
            NonHierarchicalMainRoute::StagedPlanExecution => Self::StagedPlanExecution,
            NonHierarchicalMainRoute::SingleAgentOuterLoop => Self::SingleAgentOuterLoop,
        }
    }
}

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
            resolve_non_hierarchical_main_route(&cfg, true),
            NonHierarchicalMainRoute::LogicalDualAgentStaged
        );
        assert_eq!(
            TurnOrchestrationMode::from(resolve_non_hierarchical_main_route(&cfg, true)),
            TurnOrchestrationMode::LogicalDualAgentStaged
        );
    }

    #[test]
    fn staged_when_dual_disabled_but_staged_on() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent, true);
        assert_eq!(
            resolve_non_hierarchical_main_route(&cfg, true),
            NonHierarchicalMainRoute::StagedPlanExecution
        );
    }

    #[test]
    fn single_outer_when_gate_denies() {
        let cfg = cfg_with(PlannerExecutorMode::LogicalDualAgent, true);
        assert_eq!(
            resolve_non_hierarchical_main_route(&cfg, false),
            NonHierarchicalMainRoute::SingleAgentOuterLoop
        );
    }

    #[test]
    fn single_outer_when_staged_off_even_if_gate_allows() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent, false);
        assert_eq!(
            resolve_non_hierarchical_main_route(&cfg, true),
            NonHierarchicalMainRoute::SingleAgentOuterLoop
        );
    }

    #[test]
    fn entry_resolution_denied_gate_carries_staged_reason() {
        let cfg = cfg_with(PlannerExecutorMode::LogicalDualAgent, true);
        let gate = StagedPlanningGateOutcome::Deny {
            reason: StagedPlanningDenyReason::EmptyEffectiveTask,
            task_preview: None,
            intent_decision: None,
        };
        let r = NonHierarchicalEntryResolution::resolve(&cfg, &gate);
        assert_eq!(r.main_route, NonHierarchicalMainRoute::SingleAgentOuterLoop);
        assert_eq!(
            r.single_agent_outer_loop_because,
            Some(SingleAgentOuterLoopBecause::StagedIntentGateDenied(
                StagedPlanningDenyReason::EmptyEffectiveTask
            ))
        );
    }

    #[test]
    fn entry_resolution_allow_but_single_route_is_config_fallback() {
        let cfg = cfg_with(PlannerExecutorMode::SingleAgent, false);
        use crate::agent::intent_pipeline::{IntentAction, IntentDecision};
        use crate::agent::intent_router::IntentKind;
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
            },
        };
        let r = NonHierarchicalEntryResolution::resolve(&cfg, &gate);
        assert_eq!(r.main_route, NonHierarchicalMainRoute::SingleAgentOuterLoop);
        assert_eq!(
            r.single_agent_outer_loop_because,
            Some(SingleAgentOuterLoopBecause::ConfigFallbackSingleAgentOuterLoop)
        );
    }

    #[test]
    fn entry_resolution_logical_dual_no_single_because_field() {
        let cfg = cfg_with(PlannerExecutorMode::LogicalDualAgent, true);
        use crate::agent::intent_pipeline::{IntentAction, IntentDecision};
        use crate::agent::intent_router::IntentKind;
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
            },
        };
        let r = NonHierarchicalEntryResolution::resolve(&cfg, &gate);
        assert_eq!(
            r.main_route,
            NonHierarchicalMainRoute::LogicalDualAgentStaged
        );
        assert!(r.single_agent_outer_loop_because.is_none());
    }
}
