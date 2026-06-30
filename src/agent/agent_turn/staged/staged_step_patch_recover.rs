//! 分阶段步失败后**统一**补丁规划恢复循环（outer 失败 / 验收失败 / 工具未全 ok）。

use log::debug;

use crate::agent::plan_artifact::PlanStepV1;
use crate::types::Message;

use super::patch_planner::{
    StagedPlanPatchPlannerCtx, StagedPlanStepFailureFeedbackMeta,
    push_patch_replan_assistant_json_and_notice, run_staged_plan_patch_planner_round,
    staged_patch_merged_plan_unchanged, staged_plan_step_failure_feedback_user_body,
};
use super::step_iteration_fsm::StagedStepIterationCtl;
use super::step_patch_route_fsm::{
    StagedStepPatchFailureKind, StagedStepPatchFeedbackCtx, staged_step_patch_failure_feedback,
};
use super::turn_orchestrator_fsm::StagedTurnOrchestratorPhase;

use super::super::errors::RunAgentTurnError;

/// 单次补丁恢复所需的 IO 上下文（与失败种类正交）。
pub(crate) struct StagedStepPatchRecoverBundles<'a, 'b, 'c, F> {
    pub plan_id: &'a str,
    pub i: usize,
    pub n: usize,
    pub completed_steps: usize,
    pub plan_steps: &'a mut Vec<PlanStepV1>,
    pub echo_terminal_staged: bool,
    pub patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    pub step: &'a PlanStepV1,
    pub step_user_index: usize,
}

/// 补丁恢复策略（失败种类、预算、`tracing` 相位名）。
pub(crate) struct StagedStepPatchRecoverSpec<'a> {
    pub failure_kind: StagedStepPatchFailureKind,
    pub steps_loop_phase: &'static str,
    pub patch_budget: usize,
    pub outer_loop_error_text: Option<&'a str>,
}

/// 有界补丁规划轮：成功则 `Ok(Some(RetryCurrentStep))`，否则 `Ok(None)`。
pub(crate) async fn staged_step_try_patch_recover<F>(
    bundles: StagedStepPatchRecoverBundles<'_, '_, '_, F>,
    spec: StagedStepPatchRecoverSpec<'_>,
) -> Result<Option<StagedStepIterationCtl>, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedStepPatchRecoverBundles {
        plan_id,
        i,
        mut n,
        completed_steps,
        plan_steps,
        echo_terminal_staged,
        patch_ctx,
        step,
        step_user_index,
    } = bundles;
    let effective_acceptance = crate::agent::acceptance::effective_plan_step_acceptance(step);
    let audit_footer = patch_ctx
        .per_coord
        .staged_plan_patch_vs_plan_rewrite_counters_footer();
    let mut recovered = false;
    for (attempt_idx, _) in (0..spec.patch_budget).enumerate() {
        let attempt_1based = attempt_idx.saturating_add(1);
        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "steps_loop",
            steps_loop_phase = spec.steps_loop_phase,
            staged_turn_orchestrator_phase = StagedTurnOrchestratorPhase::PatchReplanner.as_str(),
            patch_failure_kind = spec.failure_kind.as_str(),
            plan_id,
            step_index = i,
            patch_attempt = attempt_1based,
            patch_budget = spec.patch_budget,
            sub_phase = "planner",
            "staged step patch planner attempt"
        );
        let feedback_ctx = StagedStepPatchFeedbackCtx {
            outer_loop_error_text: spec.outer_loop_error_text,
            acceptance: effective_acceptance.as_ref(),
            messages: patch_ctx.p.turn.messages(),
            step_user_index,
        };
        let (detail_owned, reason_zh) =
            staged_step_patch_failure_feedback(&spec.failure_kind, feedback_ctx);
        let meta = StagedPlanStepFailureFeedbackMeta {
            plan_id,
            step_zero_based: i,
            n_steps_total: n,
            plan_patch_attempt_one_based: attempt_1based,
            plan_patch_budget: spec.patch_budget,
            reason_zh,
            detail: detail_owned.as_str(),
            audit_counters_footer: &audit_footer,
        };
        let feedback = staged_plan_step_failure_feedback_user_body(&meta, step);
        if let Some(merged) =
            run_staged_plan_patch_planner_round(patch_ctx, feedback, plan_steps.as_slice(), i)
                .await?
        {
            if staged_patch_merged_plan_unchanged(plan_steps.as_slice(), merged.as_slice()) {
                debug!(
                    target: "crabmate::staged",
                    "分阶段补丁：规划器返回与当前完全相同的 steps，停止补丁重试"
                );
                break;
            }
            *plan_steps = merged;
            n = plan_steps.len();
            push_patch_replan_assistant_json_and_notice(
                patch_ctx.p,
                plan_steps.as_slice(),
                echo_terminal_staged,
                completed_steps,
            )
            .await?;
            recovered = true;
            break;
        }
    }
    if recovered {
        return Ok(Some(StagedStepIterationCtl::RetryCurrentStep { n }));
    }
    Ok(None)
}
