//! transition 控制流跳转分支：截断/追加 `steps` 后收尾 SSE 并推进游标。

use std::collections::HashMap;

use tokio::sync::mpsc;

use crate::agent::plan_artifact::{AgentReplyPlanV1, PlanStepV1};
use crate::types::Message;

use super::patch_planner::StagedPlanPatchPlannerCtx;
use super::sse::{
    emit_chat_ui_separator_sse, finish_staged_plan_step_sse, send_staged_plan_notice,
    staged_plan_queue_summary_text,
};
use super::step_loop::{StagedStepIterationCtl, try_apply_staged_plan_control_flow_jump};

/// 可变字段聚合（与 [`CfJumpMeta`] 分拆以降低函数形参个数并避免与 `patch_ctx` 的借用冲突）。
pub(super) struct CfJumpMut<'a, 'b, 'c, F> {
    pub patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    pub plan_steps: &'a mut Vec<PlanStepV1>,
    pub transition_counters: &'a mut HashMap<String, u32>,
}

/// 控制流跳转所需的只读元数据。
pub(super) struct CfJumpMeta<'a> {
    pub original_steps: &'a [PlanStepV1],
    pub step_loop_index: usize,
    pub step_display_index: usize,
    pub completed_steps: usize,
    pub run_step: &'a Result<(), crate::agent::agent_turn::errors::RunAgentTurnError>,
    pub step_verify_failed_reason: &'a Option<String>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub plan_id: &'a str,
    pub echo_terminal_staged: bool,
}

pub(super) async fn staged_step_maybe_return_on_control_flow_jump<F>(
    bundles: CfJumpMut<'_, '_, '_, F>,
    step: &PlanStepV1,
    meta: CfJumpMeta<'_>,
) -> Option<StagedStepIterationCtl>
where
    F: Fn(String) -> Message,
{
    let (fb, step_status) = try_apply_staged_plan_control_flow_jump(
        step,
        meta.step_loop_index,
        bundles.plan_steps,
        meta.original_steps,
        bundles.transition_counters,
        meta.run_step.is_err() || meta.step_verify_failed_reason.is_some(),
        meta.step_verify_failed_reason,
    )?;
    let n = bundles.plan_steps.len();
    bundles
        .patch_ctx
        .p
        .turn
        .push_message(Message::user_staged_orchestration_injection(fb));
    let replan = AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".to_string(),
        version: 1,
        steps: bundles.plan_steps.clone(),
        no_task: false,
    };
    send_staged_plan_notice(
        meta.out,
        meta.echo_terminal_staged,
        true,
        staged_plan_queue_summary_text(&replan, meta.completed_steps),
    )
    .await;
    let step_verify_fail_reason = meta.step_verify_failed_reason.as_deref();
    finish_staged_plan_step_sse(
        meta.out,
        meta.plan_id,
        step.id.trim(),
        meta.step_display_index,
        n,
        step_status,
        step.executor_kind,
        step_verify_fail_reason,
    )
    .await;
    bundles
        .patch_ctx
        .p
        .turn
        .push_message(Message::chat_ui_separator(true));
    emit_chat_ui_separator_sse(meta.out, true).await;
    Some(StagedStepIterationCtl::AdvanceToNextStep {
        n,
        completed_steps: meta.step_display_index,
    })
}
