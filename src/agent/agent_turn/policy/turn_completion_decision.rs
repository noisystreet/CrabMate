//! [`TurnCompletionDecision`] 与 evaluate 入口（供 tracing / 金样回归）。

use crate::cm_types::{Message, ToolCall};

use super::completion_suppression::tool_calls_are_redundant_when_goal_satisfied;

/// 完成判定结果（结构化日志与金样对齐）。
///
/// **没有 `AllowEarlyStop` / `DenyEarlyStop` 变体**：#1415 之后早停门不再由同步纯逻辑判定
/// （领域检测器已删除，见 [`crate::agent::agent_turn::policy::task_level_evidence`]），
/// 它只消费「本回合是否已确认完成」这一个布尔结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnCompletionDecision {
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
            Self::AllowRedundantTools => "allow_redundant_tools",
            Self::DenyRedundantTools { .. } => "deny_redundant_tools",
            Self::AllowMissingFinalAnswerFeedback => "allow_missing_final_answer_feedback",
            Self::DenyMissingFinalAnswerFeedback { .. } => "deny_missing_final_answer_feedback",
        }
    }

    pub fn deny_reason(self) -> Option<&'static str> {
        match self {
            Self::DenyRedundantTools { reason } | Self::DenyMissingFinalAnswerFeedback { reason } => {
                Some(reason)
            }
            _ => None,
        }
    }

    pub fn is_allow(self) -> bool {
        matches!(
            self,
            Self::AllowRedundantTools | Self::AllowMissingFinalAnswerFeedback
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
