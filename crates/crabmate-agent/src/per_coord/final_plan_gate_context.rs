//! 终答 Gate **只读 Context**：`require_plan` 与来源 reason 的单点解析（与 staged 轨边界分离）。

use super::final_plan_gate::FinalPlanGatePhase;
use super::{FinalPlanRequirementMode, PlanRequirementSource};

/// 为何当前终答须（或无须）嵌入结构化规划。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalPlanRequirePlanReason {
    PolicyNever,
    PolicyAlways,
    WorkflowReflectionActive,
    NoActiveRequirement,
}

impl FinalPlanRequirePlanReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::PolicyNever => "policy_never",
            Self::PolicyAlways => "policy_always",
            Self::WorkflowReflectionActive => "workflow_reflection_active",
            Self::NoActiveRequirement => "no_active_requirement",
        }
    }
}

/// 进入 [`super::final_plan_gate::run_final_plan_gate`] 前的只读快照。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FinalPlanGateContext {
    pub require_plan: bool,
    pub phase: FinalPlanGatePhase,
    pub require_plan_reason: FinalPlanRequirePlanReason,
}

impl FinalPlanGateContext {
    pub(crate) fn apply_layer_semantics(self) -> bool {
        matches!(
            self.require_plan_reason,
            FinalPlanRequirePlanReason::WorkflowReflectionActive
        )
    }
}

pub(crate) fn build_final_plan_gate_context(
    policy: FinalPlanRequirementMode,
    source: PlanRequirementSource,
) -> FinalPlanGateContext {
    let (require_plan, reason) = match policy {
        FinalPlanRequirementMode::Never => (false, FinalPlanRequirePlanReason::PolicyNever),
        FinalPlanRequirementMode::Always => (true, FinalPlanRequirePlanReason::PolicyAlways),
        FinalPlanRequirementMode::WorkflowReflection => {
            if source == PlanRequirementSource::WorkflowReflection {
                (true, FinalPlanRequirePlanReason::WorkflowReflectionActive)
            } else {
                (false, FinalPlanRequirePlanReason::NoActiveRequirement)
            }
        }
    };
    let phase = if require_plan {
        FinalPlanGatePhase::CheckStructuredPlan
    } else {
        FinalPlanGatePhase::NoRequirement
    };
    FinalPlanGateContext {
        require_plan,
        phase,
        require_plan_reason: reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_reflection_only_when_source_active() {
        let ctx = build_final_plan_gate_context(
            FinalPlanRequirementMode::WorkflowReflection,
            PlanRequirementSource::WorkflowReflection,
        );
        assert!(ctx.require_plan);
        assert_eq!(
            ctx.require_plan_reason,
            FinalPlanRequirePlanReason::WorkflowReflectionActive
        );
        assert!(ctx.apply_layer_semantics());
    }

    #[test]
    fn workflow_reflection_policy_without_source() {
        let ctx = build_final_plan_gate_context(
            FinalPlanRequirementMode::WorkflowReflection,
            PlanRequirementSource::None,
        );
        assert!(!ctx.require_plan);
        assert_eq!(ctx.phase, FinalPlanGatePhase::NoRequirement);
    }
}
