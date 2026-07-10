//! 分阶段步执行失败后的 SSE 收尾与 `StepRetryExhausted` 文案（从 `steps_loop` 拆出以控制文件行数）。

use tokio::sync::mpsc;

use crate::agent::plan_artifact::PlanStepV1;
use crate::types::Message;

use super::super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::empty_execution::staged_step_retry_exhausted_message_body;
use super::patch_planner::StagedPlanPatchPlannerCtx;
use super::sse::{finish_staged_plan_step_sse, send_staged_plan_finished};
use super::step_loop::{StagedStepIterationCtl, staged_step_failure_retry_exhausted_message};

/// 执行步失败早退：`step_finished(failed)` + `plan_finished(failed)`，避免漏发 `staged_plan_finished`。
pub(super) struct StagedPlanStepFailedExit<'a> {
    pub out: Option<&'a mpsc::Sender<String>>,
    pub plan_id: &'a str,
    pub step_id_trim: &'a str,
    pub step_index: usize,
    pub n: usize,
    pub completed_steps_before_this: usize,
}

pub(super) async fn finish_staged_plan_step_failed_and_plan_failed_sse(
    f: StagedPlanStepFailedExit<'_>,
    executor_kind: Option<crate::agent::plan_artifact::PlanStepExecutorKind>,
    verify_fail_reason: Option<&str>,
) {
    finish_staged_plan_step_sse(
        f.out,
        f.plan_id,
        f.step_id_trim,
        f.step_index,
        f.n,
        "failed",
        executor_kind,
        verify_fail_reason,
    )
    .await;
    send_staged_plan_finished(
        f.out,
        f.plan_id,
        f.n,
        f.completed_steps_before_this,
        "failed",
    )
    .await;
}

pub(super) struct StagedStepOuterFailureExitParams<'a, 'b, 'c, F> {
    pub out: Option<&'a mpsc::Sender<String>>,
    pub plan_id: &'a str,
    pub step: &'a PlanStepV1,
    pub step_index: usize,
    pub step_user_idx: usize,
    pub n: usize,
    pub completed_steps: usize,
    pub run_step: &'a Result<(), RunAgentTurnError>,
    pub step_verify_failed_reason: &'a Option<String>,
    pub patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
}

pub(super) async fn staged_step_fail_after_outer_execution_exhausted<F>(
    p: StagedStepOuterFailureExitParams<'_, '_, '_, F>,
) -> Result<StagedStepIterationCtl, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedStepOuterFailureExitParams {
        out,
        plan_id,
        step,
        step_index,
        step_user_idx,
        n,
        completed_steps,
        run_step,
        step_verify_failed_reason,
        patch_ctx,
    } = p;
    finish_staged_plan_step_failed_and_plan_failed_sse(
        StagedPlanStepFailedExit {
            out,
            plan_id,
            step_id_trim: step.id.trim(),
            step_index,
            n,
            completed_steps_before_this: completed_steps,
        },
        step.executor_kind,
        step_verify_failed_reason.as_deref(),
    )
    .await;

    let user_goal = patch_ctx
        .p
        .turn
        .staged_immutable_user_goal_snapshot()
        .map(str::to_string);
    let audit_footer = patch_ctx
        .per_coord
        .staged_plan_patch_vs_plan_rewrite_counters_footer();
    let reason = staged_step_retry_exhausted_message_body(
        staged_step_failure_retry_exhausted_message(run_step, step_verify_failed_reason),
        patch_ctx.p.turn.messages(),
        step_user_idx,
        user_goal.as_deref(),
        audit_footer.as_str(),
    );
    Err(RunAgentTurnError::StepRetryExhausted {
        phase: AgentTurnSubPhase::Executor,
        message: reason,
    })
}
