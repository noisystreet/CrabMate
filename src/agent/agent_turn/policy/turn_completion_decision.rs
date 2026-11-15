//! [`TurnCompletionDecision`] 与 evaluate 入口（供 tracing / 金样回归）。

use crate::cm_agent::plan_artifact::PlanStepV1;
use crate::cm_types::{Message, ToolCall};

use super::completion_suppression::{
    plan_steps_are_redundant_after_completion, plan_steps_require_formal_execution,
    tool_calls_are_redundant_when_goal_satisfied,
};
use super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
    generic_task_intent_implies_build_or_test,
};

/// 完成判定结果（结构化日志与金样对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCompletionDecision {
    AllowEarlyStop,
    DenyEarlyStop {
        reason: &'static str,
    },
    /// 金样 / [`evaluate_turn_suppress_replanning`]；生产 outer_loop 尚未接线。
    AllowSuppressReplanning,
    /// 见 `AllowSuppressReplanning`。
    DenySuppressReplanning {
        reason: &'static str,
    },
    AllowRedundantTools,
    DenyRedundantTools {
        reason: &'static str,
    },
    AllowMissingFinalAnswerFeedback,
    DenyMissingFinalAnswerFeedback {
        reason: &'static str,
    },
}

impl TurnCompletionDecision {
    pub fn as_trace_str(self) -> &'static str {
        match self {
            Self::AllowEarlyStop => "allow_early_stop",
            Self::DenyEarlyStop { .. } => "deny_early_stop",
            Self::AllowSuppressReplanning => "allow_suppress_replanning",
            Self::DenySuppressReplanning { .. } => "deny_suppress_replanning",
            Self::AllowRedundantTools => "allow_redundant_tools",
            Self::DenyRedundantTools { .. } => "deny_redundant_tools",
            Self::AllowMissingFinalAnswerFeedback => "allow_missing_final_answer_feedback",
            Self::DenyMissingFinalAnswerFeedback { .. } => "deny_missing_final_answer_feedback",
        }
    }

    pub fn deny_reason(self) -> Option<&'static str> {
        match self {
            Self::DenyEarlyStop { reason }
            | Self::DenySuppressReplanning { reason }
            | Self::DenyRedundantTools { reason }
            | Self::DenyMissingFinalAnswerFeedback { reason } => Some(reason),
            _ => None,
        }
    }

    pub fn is_allow(self) -> bool {
        matches!(
            self,
            Self::AllowEarlyStop
                | Self::AllowSuppressReplanning
                | Self::AllowRedundantTools
                | Self::AllowMissingFinalAnswerFeedback
        )
    }
}

pub fn log_turn_completion_decision(decision: TurnCompletionDecision, check: &'static str) {
    tracing::debug!(
        target: "crabmate::agent_turn",
        turn_completion_check = check,
        turn_completion_decision = decision.as_trace_str(),
        turn_completion_deny_reason = decision.deny_reason(),
        "turn_completion decision"
    );
}

/// 消息历史里是否存在**强**验证证据（专用工具 + 结构化退出码）。
///
/// 只用于「构建/测试类任务的早停该不该放行」这一处判断；采集逻辑在
/// [`super::task_level_evidence::EvidenceSet`]，本函数只是取一个视图。
fn has_strong_verify_evidence(messages: &[Message]) -> bool {
    super::task_level_evidence::EvidenceSet::collect(messages).has_strong_verify_success()
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
    let Some(task) = crate::cm_types::last_real_user_task_content(messages, false) else {
        return TurnCompletionDecision::DenyEarlyStop {
            reason: "no_active_user_task",
        };
    };
    // 「任务像构建/测试」曾是无条件拒绝早停的保守闸。它的本意是对的——构建类任务的
    // 完成最易误判——但它把**证据强度**一起忽略了：`run_command` 跑 `make` 只能靠
    // `built target` 之类文本特征（弱证据），保守拒绝合理；而 `cargo_test` 带结构化
    // `exit_code`（强证据）不该同等对待，否则「越规范地用专用工具，越判不出完成」。
    // 因此：**弱证据维持拒绝，强证据放行**。
    if generic_task_intent_implies_build_or_test(task) && !has_strong_verify_evidence(messages) {
        TurnCompletionDecision::DenyEarlyStop {
            reason: "build_or_test_intent",
        }
    } else {
        TurnCompletionDecision::AllowEarlyStop
    }
}

pub fn evaluate_turn_early_stop(messages: &[Message]) -> TurnCompletionDecision {
    let decision = turn_early_stop_allowed_core(messages);
    log_turn_completion_decision(decision, "early_stop");
    decision
}

/// 步后抑规划：目标已 Satisfied 且新 `steps` 仅为探针/总结时是否应抑制下一轮无工具规划。
///
/// 生产 outer_loop 尚未调用；由 **`fixtures/turn_completion_golden.jsonl`** 与根包包装测覆盖。
pub fn evaluate_turn_suppress_replanning(
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

pub fn evaluate_turn_redundant_tools(
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
