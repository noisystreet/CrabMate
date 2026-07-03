//! 外循环 **Gate 前** L2 纠偏原因（`outer_loop_reflect`；与 [`crate::agent::per_coord::final_plan_gate_reason`] 正交）。
//!
//! 当 [`super::reflect::ReflectOnAssistantOutcome::StopTurn`] 且 Gate 已允许结束，但用户目标仍缺构建进展或终答形态不足时，
//! 注入编排 user 并 **`ContinueOuter`**；**不**替代终答 Gate 决策。

/// Gate 前外循环纠偏注入原因（仅 `tracing` / 单测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OuterLoopReflectPreGateReason {
    /// 构建/编译意图下 assistant 空转无工具（`outer_loop_build_idle`）。
    BuildIdleFeedback,
    /// 有工具成功但终答未给出可验收结论（`outer_loop_missing_final_answer`）。
    MissingFinalAnswerFeedback,
}

impl OuterLoopReflectPreGateReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::BuildIdleFeedback => "build_idle_feedback",
            Self::MissingFinalAnswerFeedback => "missing_final_answer_feedback",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OuterLoopReflectPreGateReason;

    #[test]
    fn pre_gate_reason_trace_strings_stable() {
        assert_eq!(
            OuterLoopReflectPreGateReason::BuildIdleFeedback.as_str(),
            "build_idle_feedback"
        );
        assert_eq!(
            OuterLoopReflectPreGateReason::MissingFinalAnswerFeedback.as_str(),
            "missing_final_answer_feedback"
        );
    }
}
