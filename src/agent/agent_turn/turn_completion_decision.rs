//! [`TurnCompletionDecision`] 与 evaluate 入口（供 tracing / 金样回归）。

use crate::agent::plan_artifact::{PlanStepAcceptance, PlanStepV1};
use crate::types::{Message, ToolCall, last_staged_step_injection_index};

use super::super::completion_suppression::{
    plan_steps_are_redundant_after_completion, plan_steps_require_formal_execution,
    tool_calls_are_redundant_when_goal_satisfied,
};
use super::super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test,
};

/// 完成判定结果（结构化日志与金样对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnCompletionDecision {
    AllowEarlyStop,
    DenyEarlyStop { reason: &'static str },
    AllowSuppressReplanning,
    DenySuppressReplanning { reason: &'static str },
    AllowRedundantTools,
    DenyRedundantTools { reason: &'static str },
    AllowRollingHorizonStop { via: RollingHorizonStopVia },
    DenyRollingHorizonStop { reason: &'static str },
    AllowMissingFinalAnswerFeedback,
    DenyMissingFinalAnswerFeedback { reason: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RollingHorizonStopVia {
    HeuristicEarlyStop,
    StepAcceptancePass,
}

impl TurnCompletionDecision {
    pub(crate) fn as_trace_str(self) -> &'static str {
        match self {
            Self::AllowEarlyStop => "allow_early_stop",
            Self::DenyEarlyStop { .. } => "deny_early_stop",
            Self::AllowSuppressReplanning => "allow_suppress_replanning",
            Self::DenySuppressReplanning { .. } => "deny_suppress_replanning",
            Self::AllowRedundantTools => "allow_redundant_tools",
            Self::DenyRedundantTools { .. } => "deny_redundant_tools",
            Self::AllowRollingHorizonStop { .. } => "allow_rolling_horizon_stop",
            Self::DenyRollingHorizonStop { .. } => "deny_rolling_horizon_stop",
            Self::AllowMissingFinalAnswerFeedback => "allow_missing_final_answer_feedback",
            Self::DenyMissingFinalAnswerFeedback { .. } => "deny_missing_final_answer_feedback",
        }
    }

    pub(crate) fn deny_reason(self) -> Option<&'static str> {
        match self {
            Self::DenyEarlyStop { reason }
            | Self::DenySuppressReplanning { reason }
            | Self::DenyRedundantTools { reason }
            | Self::DenyRollingHorizonStop { reason }
            | Self::DenyMissingFinalAnswerFeedback { reason } => Some(reason),
            _ => None,
        }
    }

    pub(crate) fn is_allow(self) -> bool {
        matches!(
            self,
            Self::AllowEarlyStop
                | Self::AllowSuppressReplanning
                | Self::AllowRedundantTools
                | Self::AllowRollingHorizonStop { .. }
                | Self::AllowMissingFinalAnswerFeedback
        )
    }

    pub(crate) fn rolling_horizon_via(self) -> Option<RollingHorizonStopVia> {
        match self {
            Self::AllowRollingHorizonStop { via } => Some(via),
            _ => None,
        }
    }
}

pub(crate) fn log_turn_completion_decision(decision: TurnCompletionDecision, check: &'static str) {
    tracing::debug!(
        target: "crabmate::agent_turn",
        turn_completion_check = check,
        turn_completion_decision = decision.as_trace_str(),
        turn_completion_deny_reason = decision.deny_reason(),
        "turn_completion decision"
    );
}

fn turn_early_stop_allowed_core(messages: &[Message]) -> TurnCompletionDecision {
    if !matches!(
        check_active_user_goal_completion_evidence(messages),
        GoalCompletionEvidenceCheck::Satisfied
    ) {
        return TurnCompletionDecision::DenyEarlyStop {
            reason: "evidence_not_satisfied",
        };
    }
    let Some(task) = crate::agent::plan_optimizer::staged_plan_trigger_user_content(messages)
    else {
        return TurnCompletionDecision::DenyEarlyStop {
            reason: "no_active_user_task",
        };
    };
    if generic_task_intent_implies_build_or_test(task) {
        TurnCompletionDecision::DenyEarlyStop {
            reason: "build_or_test_intent",
        }
    } else {
        TurnCompletionDecision::AllowEarlyStop
    }
}

pub(crate) fn evaluate_turn_early_stop(messages: &[Message]) -> TurnCompletionDecision {
    let decision = turn_early_stop_allowed_core(messages);
    log_turn_completion_decision(decision, "early_stop");
    decision
}

pub(crate) fn evaluate_turn_suppress_replanning(
    messages: &[Message],
    entered_from_step_execution_round: bool,
    steps: &[PlanStepV1],
) -> TurnCompletionDecision {
    let decision = if !entered_from_step_execution_round {
        TurnCompletionDecision::DenySuppressReplanning {
            reason: "not_from_step_execution_round",
        }
    } else if steps.is_empty() {
        TurnCompletionDecision::DenySuppressReplanning {
            reason: "empty_steps",
        }
    } else if plan_steps_require_formal_execution(steps) {
        TurnCompletionDecision::DenySuppressReplanning {
            reason: "formal_execution_required",
        }
    } else if !turn_early_stop_allowed_core(messages).is_allow() {
        TurnCompletionDecision::DenySuppressReplanning {
            reason: "early_stop_not_allowed",
        }
    } else if !plan_steps_are_redundant_after_completion(steps) {
        TurnCompletionDecision::DenySuppressReplanning {
            reason: "steps_not_redundant",
        }
    } else {
        TurnCompletionDecision::AllowSuppressReplanning
    };
    log_turn_completion_decision(decision, "suppress_replanning");
    decision
}

pub(crate) fn evaluate_turn_redundant_tools(
    tool_calls: &[ToolCall],
    messages: &[Message],
) -> TurnCompletionDecision {
    let decision = if !tool_calls_are_redundant_when_goal_satisfied(tool_calls, messages) {
        TurnCompletionDecision::DenyRedundantTools {
            reason: "tools_not_redundant",
        }
    } else if !turn_early_stop_allowed_core(messages).is_allow() {
        TurnCompletionDecision::DenyRedundantTools {
            reason: "early_stop_not_allowed",
        }
    } else {
        TurnCompletionDecision::AllowRedundantTools
    };
    log_turn_completion_decision(decision, "redundant_tools");
    decision
}

pub(crate) fn evaluate_turn_staged_rolling_horizon_early_stop(
    messages: &[Message],
    last_completed_step_effective_acceptance: Option<&PlanStepAcceptance>,
    workspace_root: &std::path::Path,
) -> TurnCompletionDecision {
    if matches!(
        turn_early_stop_allowed_core(messages),
        TurnCompletionDecision::AllowEarlyStop
    ) {
        let decision = TurnCompletionDecision::AllowRollingHorizonStop {
            via: RollingHorizonStopVia::HeuristicEarlyStop,
        };
        log_turn_completion_decision(decision, "rolling_horizon_early_stop");
        return decision;
    }
    let Some(acceptance) = last_completed_step_effective_acceptance else {
        let decision = TurnCompletionDecision::DenyRollingHorizonStop {
            reason: "no_last_step_acceptance",
        };
        log_turn_completion_decision(decision, "rolling_horizon_early_stop");
        return decision;
    };
    let Some(step_idx) = last_staged_step_injection_index(messages) else {
        let decision = TurnCompletionDecision::DenyRollingHorizonStop {
            reason: "no_staged_step_injection",
        };
        log_turn_completion_decision(decision, "rolling_horizon_early_stop");
        return decision;
    };
    let decision = if crate::agent::step_verifier::verify_step_execution(
        acceptance,
        messages,
        step_idx,
        workspace_root,
    )
    .is_pass()
    {
        TurnCompletionDecision::AllowRollingHorizonStop {
            via: RollingHorizonStopVia::StepAcceptancePass,
        }
    } else {
        TurnCompletionDecision::DenyRollingHorizonStop {
            reason: "step_acceptance_not_pass",
        }
    };
    log_turn_completion_decision(decision, "rolling_horizon_early_stop");
    decision
}
