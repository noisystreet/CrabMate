//! 分阶段步失败补丁路径的纯路由：失败种类 → 反馈文案与 `reason_zh`（**不**调用 LLM）。

use crate::agent::plan_artifact::PlanStepAcceptance;

use super::empty_execution::{
    staged_step_empty_execution_is_reason, staged_step_empty_execution_patch_detail,
};
use super::step_iteration_fsm::{
    STAGED_STEP_OUTER_LOOP_FAIL_DETAIL, staged_step_exec_fail_patch_detail,
    staged_step_verify_fail_patch_detail,
};

/// 步失败后进入补丁规划员前的失败分类（与 `steps_loop` 两条恢复路径对齐）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedStepPatchFailureKind {
    /// `run_agent_outer_loop` 返回 `Err`。
    OuterLoopError,
    /// `step_verifier` 或空执行检测失败。
    StepVerifyFail {
        reason: String,
        empty_execution: bool,
    },
}

/// 由 outer 结果与验收失败原因解析补丁失败种类（无 patch 时由调用方短路）。
pub(crate) fn resolve_staged_step_patch_failure_kind(
    step_verify_failed_reason: &Option<String>,
    has_outer_loop_error: bool,
) -> Option<StagedStepPatchFailureKind> {
    if let Some(vr) = step_verify_failed_reason {
        return Some(StagedStepPatchFailureKind::StepVerifyFail {
            reason: vr.clone(),
            empty_execution: staged_step_empty_execution_is_reason(vr),
        });
    }
    if has_outer_loop_error {
        return Some(StagedStepPatchFailureKind::OuterLoopError);
    }
    None
}

/// 补丁规划 **user** 正文的 `detail` 与 `reason_zh`（表驱动，与历史 `steps_loop` 文案一致）。
pub(crate) fn staged_step_patch_failure_feedback(
    kind: &StagedStepPatchFailureKind,
    outer_loop_error_text: Option<&str>,
    acceptance: Option<&PlanStepAcceptance>,
) -> (String, &'static str) {
    match kind {
        StagedStepPatchFailureKind::OuterLoopError => {
            let detail = outer_loop_error_text
                .map(staged_step_exec_fail_patch_detail)
                .unwrap_or_else(|| STAGED_STEP_OUTER_LOOP_FAIL_DETAIL.to_string());
            (detail, "执行子循环返回错误")
        }
        StagedStepPatchFailureKind::StepVerifyFail {
            reason,
            empty_execution,
        } => {
            let detail = if *empty_execution {
                staged_step_empty_execution_patch_detail(reason, acceptance)
            } else {
                staged_step_verify_fail_patch_detail(reason, acceptance)
            };
            (detail, "本步确定性验证失败 (Step Verification Failed)")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_verify_fail_kind() {
        let kind =
            resolve_staged_step_patch_failure_kind(&Some("exit_code_mismatch".into()), false)
                .expect("kind");
        assert_eq!(
            kind,
            StagedStepPatchFailureKind::StepVerifyFail {
                reason: "exit_code_mismatch".into(),
                empty_execution: false,
            }
        );
    }

    #[test]
    fn resolve_outer_error_kind() {
        assert_eq!(
            resolve_staged_step_patch_failure_kind(&None, true),
            Some(StagedStepPatchFailureKind::OuterLoopError)
        );
    }

    #[test]
    fn feedback_outer_loop_error() {
        let kind = StagedStepPatchFailureKind::OuterLoopError;
        let (detail, reason) = staged_step_patch_failure_feedback(&kind, Some("boom"), None);
        assert!(detail.contains("boom"));
        assert_eq!(reason, "执行子循环返回错误");
    }
}
