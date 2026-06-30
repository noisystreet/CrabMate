//! 分阶段步失败补丁路径的纯路由：失败种类 → 反馈文案与 `reason_zh`（**不**调用 LLM）。

use crate::agent::plan_artifact::PlanStepAcceptance;
use crate::types::Message;

use super::empty_execution::{
    staged_step_empty_execution_is_reason, staged_step_empty_execution_patch_detail,
};
use super::step_iteration_fsm::{
    STAGED_STEP_OUTER_LOOP_FAIL_DETAIL, staged_step_exec_fail_patch_detail,
    staged_step_tool_failure_patch_detail, staged_step_verify_fail_patch_detail,
};

/// 步失败后进入补丁规划员前的失败分类（执行 / 验收 / 工具三条路径统一词汇）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StagedStepPatchFailureKind {
    /// `run_agent_outer_loop` 返回 `Err`。
    OuterLoopError,
    /// `step_verifier` 或空执行检测失败。
    StepVerifyFail {
        reason: String,
        empty_execution: bool,
    },
    /// 本步 `role: tool` 未全部成功。
    ToolMessagesNotOk,
}

impl StagedStepPatchFailureKind {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::OuterLoopError => "outer_loop_error",
            Self::StepVerifyFail { .. } => "step_verify_fail",
            Self::ToolMessagesNotOk => "tool_messages_not_ok",
        }
    }
}

/// 补丁反馈文案所需的只读上下文（与失败种类组合使用）。
#[derive(Debug, Clone, Copy)]
pub(crate) struct StagedStepPatchFeedbackCtx<'a> {
    pub outer_loop_error_text: Option<&'a str>,
    pub acceptance: Option<&'a PlanStepAcceptance>,
    pub messages: &'a [Message],
    pub step_user_index: usize,
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
    ctx: StagedStepPatchFeedbackCtx<'_>,
) -> (String, &'static str) {
    match kind {
        StagedStepPatchFailureKind::OuterLoopError => {
            let detail = ctx
                .outer_loop_error_text
                .map(staged_step_exec_fail_patch_detail)
                .unwrap_or_else(|| STAGED_STEP_OUTER_LOOP_FAIL_DETAIL.to_string());
            (detail, "执行子循环返回错误")
        }
        StagedStepPatchFailureKind::StepVerifyFail {
            reason,
            empty_execution,
        } => {
            let detail = if *empty_execution {
                staged_step_empty_execution_patch_detail(reason, ctx.acceptance)
            } else {
                staged_step_verify_fail_patch_detail(reason, ctx.acceptance)
            };
            (detail, "本步确定性验证失败 (Step Verification Failed)")
        }
        StagedStepPatchFailureKind::ToolMessagesNotOk => {
            let detail = staged_step_tool_failure_patch_detail(
                ctx.messages,
                ctx.step_user_index,
                ctx.acceptance,
            );
            (detail, "本步内工具调用未全部成功")
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
    fn tool_messages_kind_constant() {
        assert_eq!(
            StagedStepPatchFailureKind::ToolMessagesNotOk.as_str(),
            "tool_messages_not_ok"
        );
    }

    #[test]
    fn feedback_outer_loop_error() {
        let kind = StagedStepPatchFailureKind::OuterLoopError;
        let ctx = StagedStepPatchFeedbackCtx {
            outer_loop_error_text: Some("boom"),
            acceptance: None,
            messages: &[],
            step_user_index: 0,
        };
        let (detail, reason) = staged_step_patch_failure_feedback(&kind, ctx);
        assert!(detail.contains("boom"));
        assert_eq!(reason, "执行子循环返回错误");
    }
}
