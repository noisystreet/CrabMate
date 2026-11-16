//! [`TurnCompletionDecision`] 与 evaluate 入口（供 tracing / 金样回归）。

use crate::cm_agent::plan_artifact::PlanStepV1;
use crate::cm_types::{Message, ToolCall};

use super::completion_suppression::{
    plan_steps_are_redundant_after_completion, plan_steps_require_formal_execution,
    tool_calls_are_redundant_when_goal_satisfied,
};
use super::task_level_evidence::{
    GoalCompletionEvidenceCheck, check_active_user_goal_completion_evidence,
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

/// 完成判定门：`Satisfied` 或**已缓存确认**放行，其余一律拒绝。
///
/// 这里**没有**第二道「任务像构建/测试就拒绝」的闸。那道闸靠 `build_or_test_intent`
/// 关键词表判断（13 个子串，`"test"` 会命中 `greatest`），#1415 已把它连同第 ① 层的
/// 同类关键词路径一起移除：判定不再看任务措辞，只看结构化证据 + 语义判定。
///
/// `completion_confirmed` 是本回合**早停门**（可问 LLM）已写下的结论。冗余探针抑制与
/// 步后抑规划是同步路径、拿不到 judge，通过它复用同一结论；否则同一批证据会出现
/// 「早停门放行、抑制门不放行」的分裂。
fn turn_early_stop_allowed_core(
    messages: &[Message],
    completion_confirmed: bool,
) -> TurnCompletionDecision {
    if completion_confirmed
        || matches!(
            check_active_user_goal_completion_evidence(messages),
            GoalCompletionEvidenceCheck::Satisfied
        )
    {
        TurnCompletionDecision::AllowEarlyStop
    } else {
        TurnCompletionDecision::DenyEarlyStop {
            reason: "evidence_not_satisfied",
        }
    }
}

/// 同步入口（金样 / 未接线路径）：只看第 ① 层，`completion_confirmed` 视为 `false`。
pub fn evaluate_turn_early_stop(messages: &[Message]) -> TurnCompletionDecision {
    let decision = turn_early_stop_allowed_core(messages, false);
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
    } else if !turn_early_stop_allowed_core(messages, false).is_allow() {
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

/// 本轮工具调用是否应被静默丢弃：目标已完成 + 调用均为探针 / 重复 `run_command`。
///
/// `completion_confirmed` 由 `PerCoordinator` 传入——语义判定（judge）的结论缓存在那里，
/// 同步门据此与早停门保持同一口径（#1415）。
pub fn evaluate_turn_redundant_tools(
    tool_calls: &[ToolCall],
    messages: &[Message],
    completion_confirmed: bool,
) -> TurnCompletionDecision {
    let decision = if !tool_calls_are_redundant_when_goal_satisfied(tool_calls, messages) {
        TurnCompletionDecision::DenyRedundantTools {
            reason: "tools_not_redundant",
        }
    } else if !turn_early_stop_allowed_core(messages, completion_confirmed).is_allow() {
        TurnCompletionDecision::DenyRedundantTools {
            reason: "early_stop_not_allowed",
        }
    } else {
        TurnCompletionDecision::AllowRedundantTools
    };
    log_turn_completion_decision(decision, "redundant_tools");
    decision
}
