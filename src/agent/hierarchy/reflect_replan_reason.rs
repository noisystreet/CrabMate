//! 分层 Manager **`reflect_and_replan`** 触发原因（PER **R** 第四条独立路径；与终答 Gate / 外循环纠偏正交）。

/// Manager 反思重规划触发原因（`tracing` 字段 **`manager_reflect_replan_reason`**）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerReflectReplanReason {
    /// 子目标 `GoalVerifier` 验收失败且仍有重试预算。
    GoalVerificationFailed,
    /// Operator 返回 `NeedsDecomposition`，Manager 调整子目标而非立刻拆子图。
    NeedsDecomposition,
}

impl ManagerReflectReplanReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoalVerificationFailed => "goal_verification_failed",
            Self::NeedsDecomposition => "needs_decomposition",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ManagerReflectReplanReason;

    #[test]
    fn reflect_replan_reason_trace_strings_stable() {
        assert_eq!(
            ManagerReflectReplanReason::GoalVerificationFailed.as_str(),
            "goal_verification_failed"
        );
        assert_eq!(
            ManagerReflectReplanReason::NeedsDecomposition.as_str(),
            "needs_decomposition"
        );
    }
}
