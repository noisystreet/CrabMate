//! 外循环反思结果 → **`ReflectBranchCtl`** 映射（副作用与 IO 仍在本模块；类型见 **`outer_loop_fsm`**）。

use std::sync::atomic::Ordering;

use log::{debug, info};

use crate::agent::per_coord::PerCoordinator;
use crate::sse::{SsePayload, encode_message};
use crate::types::Message;

use super::errors::sse_plan_rewrite_exhausted_body;
use super::outer_loop_build_idle::{
    outer_loop_assistant_is_build_idle_without_tools, outer_loop_build_idle_feedback_if_needed,
    outer_loop_window_has_build_progress_since_last_user,
};
use super::outer_loop_fsm::ReflectBranchCtl;
use super::params::RunLoopParams;
use super::reflect::ReflectOnAssistantOutcome;
use super::turn_completion::outer_loop_missing_final_answer_feedback_if_needed;

/// `per_reflect_after_assistant` 结果 → 外循环控制（含 build-idle 纠偏、plan_rewrite SSE 等）。
pub(super) async fn map_reflect_outcome_to_branch_ctl(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    msg: &Message,
    outcome: ReflectOnAssistantOutcome,
) -> ReflectBranchCtl {
    match outcome {
        ReflectOnAssistantOutcome::StopTurn => {
            let messages = p.turn.messages();
            if let Some(task) =
                crate::agent::plan_optimizer::staged_plan_trigger_user_content(messages)
            {
                if outer_loop_window_has_build_progress_since_last_user(p.turn.messages()) {
                    per_coord.reset_outer_loop_build_idle_streak();
                } else if outer_loop_assistant_is_build_idle_without_tools(msg) {
                    let streak = per_coord.record_outer_loop_build_idle_round();
                    if let Some(feedback) = outer_loop_build_idle_feedback_if_needed(
                        task,
                        messages,
                        msg,
                        streak,
                        per_coord.outer_loop_build_idle_feedback_injected(),
                    ) {
                        info!(
                            target: "crabmate::agent_turn",
                            "L2 外循环构建空转纠偏：注入 user 并继续 outer_loop_build_idle_streak={streak}"
                        );
                        p.turn
                            .push_message(Message::user_staged_orchestration_injection(feedback));
                        per_coord.record_outer_loop_build_idle_feedback_injected();
                        if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                            f.sync_from_per_coord(per_coord);
                        }
                        return ReflectBranchCtl::ContinueOuter;
                    }
                }
                if let Some(feedback) = outer_loop_missing_final_answer_feedback_if_needed(
                    messages,
                    msg,
                    per_coord.outer_loop_missing_final_answer_feedback_injected(),
                ) {
                    info!(
                        target: "crabmate::agent_turn",
                        "L2 外循环终答缺失纠偏：注入 user 并继续"
                    );
                    p.turn
                        .push_message(Message::user_staged_orchestration_injection(feedback));
                    per_coord.record_outer_loop_missing_final_answer_feedback_injected();
                    if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                        f.sync_from_per_coord(per_coord);
                    }
                    return ReflectBranchCtl::ContinueOuter;
                }
            }
            if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
            }
            ReflectBranchCtl::BreakOuter
        }
        ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
            if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
                f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
            }
            ReflectBranchCtl::ContinueOuter
        }
        ReflectOnAssistantOutcome::ProceedToExecuteTools => {
            per_coord.reset_outer_loop_build_idle_streak();
            if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
            }
            ReflectBranchCtl::ProceedToTools
        }
        ReflectOnAssistantOutcome::PlanRewriteExhausted { reason } => {
            if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
            }
            if let Some(tx) = p.ctx.io.out {
                let _ = crate::sse::send_string_logged(
                    tx,
                    encode_message(SsePayload::Error(sse_plan_rewrite_exhausted_body(
                        p.ctx.obs.tracing_chat_turn.as_ref(),
                        reason.as_str(),
                    ))),
                    "outer_loop::plan_rewrite_exhausted",
                )
                .await;
            }
            ReflectBranchCtl::BreakOuter
        }
        ReflectOnAssistantOutcome::UserCancelled => {
            debug!(
                target: "crabmate::agent_turn",
                "map_reflect_outcome_to_branch_ctl: UserCancelled should be handled in outer_loop_reflect_branch"
            );
            ReflectBranchCtl::BreakOuter
        }
    }
}
