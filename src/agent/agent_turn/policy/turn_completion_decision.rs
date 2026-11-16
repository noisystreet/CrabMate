//! [`TurnCompletionDecision`] 与 evaluate 入口（供 tracing / 金样回归）。

use crate::cm_agent::plan_artifact::PlanStepV1;
use crate::cm_types::{Message, ToolCall};

use super::completion_suppression::{
    plan_steps_are_redundant_after_completion, plan_steps_require_formal_execution,
    tool_calls_are_redundant_when_goal_satisfied,
};

/// 完成判定结果（结构化日志与金样对齐）。
///
/// **没有 `AllowEarlyStop` / `DenyEarlyStop` 变体**：#1415 之后早停门不再由同步纯逻辑判定
/// （领域检测器已删除，见 [`crate::agent::agent_turn::policy::task_level_evidence`]），
/// 它只消费「本回合是否已确认完成」这一个布尔结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCompletionDecision {
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
            Self::DenySuppressReplanning { reason }
            | Self::DenyRedundantTools { reason }
            | Self::DenyMissingFinalAnswerFeedback { reason } => Some(reason),
            _ => None,
        }
    }

    pub fn is_allow(self) -> bool {
        matches!(
            self,
            Self::AllowSuppressReplanning
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

/// 步后抑规划：已完成确认 + 新 `steps` 仅为探针/总结时是否应抑制下一轮无工具规划。
///
/// 生产 outer_loop 尚未调用；由 **`fixtures/turn_completion_golden.jsonl`** 与根包包装测覆盖。
///
/// `completion_confirmed` 由 `PerCoordinator` 传入——语义判定（judge）的结论缓存在那里，
/// 本门是同步路径、拿不到 judge，只能复用它。原先是自己再跑一遍第 ① 层结构化证据，
/// 如今第 ① 层已删（#1415），两道门共用同一结论。
pub fn evaluate_turn_suppress_replanning(
    entered_from_step_execution_round: bool,
    steps: &[PlanStepV1],
    completion_confirmed: bool,
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
    } else if !completion_confirmed {
        TurnCompletionDecision::DenySuppressReplanning {
            reason: "completion_not_confirmed",
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
/// `messages` 只用于识别「是否重复了已成功的 `run_command`」（`completion_suppression`），
/// **不再用来判定完成**；完成与否只由 `completion_confirmed` 决定。
pub fn evaluate_turn_redundant_tools(
    tool_calls: &[ToolCall],
    messages: &[Message],
    completion_confirmed: bool,
) -> TurnCompletionDecision {
    let decision = if !tool_calls_are_redundant_when_goal_satisfied(tool_calls, messages) {
        TurnCompletionDecision::DenyRedundantTools {
            reason: "tools_not_redundant",
        }
    } else if !completion_confirmed {
        TurnCompletionDecision::DenyRedundantTools {
            reason: "completion_not_confirmed",
        }
    } else {
        TurnCompletionDecision::AllowRedundantTools
    };
    log_turn_completion_decision(decision, "redundant_tools");
    decision
}
