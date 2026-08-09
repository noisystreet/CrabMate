//! 外循环反思结果 → **`ReflectBranchCtl`** 映射（副作用与 IO 仍在本模块；类型见 **`outer_loop_fsm`**）。
//!
//! **Gate 前 L2 纠偏**（build-idle、终答缺失）在本模块处理；终答规划门控见 **`per_coord::final_plan_gate`**。

use std::sync::atomic::Ordering;

use log::{debug, info};

use crate::agent::per_coord::PerCoordinator;
use crate::sse::{SsePayload, encode_message};
use crate::types::Message;

use super::outer_loop_build_idle::{
    outer_loop_assistant_is_build_idle_without_tools, outer_loop_build_idle_feedback_if_needed,
    outer_loop_window_has_build_progress_since_last_user,
};
use super::outer_loop_fsm::ReflectBranchCtl;
use super::outer_loop_reflect_reason::OuterLoopReflectPreGateReason;
use super::turn_completion::outer_loop_missing_final_answer_feedback_if_needed;
use crate::agent::agent_turn::errors::sse_plan_rewrite_exhausted_body;
use crate::agent::agent_turn::params::RunLoopParams;
use crate::agent::agent_turn::reflect::ReflectOnAssistantOutcome;

fn sync_per_flight_from_coord(p: &RunLoopParams<'_>, per_coord: &PerCoordinator) {
    if let Some(f) = p.ctx.attach.per_flight.as_ref() {
        f.sync_from_per_coord(per_coord);
    }
}

fn push_pre_gate_user_feedback(p: &mut RunLoopParams<'_>, feedback: String) {
    p.turn.push_message(Message {
        role: "user".to_string(),
        content: Some(feedback.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    });
}

/// `StopTurn`：build-idle / 终答缺失纠偏 → 继续外循环，否则结束。
fn reflect_branch_on_stop_turn(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    msg: &Message,
) -> ReflectBranchCtl {
    let messages = p.turn.messages();
    if let Some(task) = crate::types::last_real_user_task_content(messages, false) {
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
                    "outer loop pre-gate: {} streak={}",
                    OuterLoopReflectPreGateReason::BuildIdleFeedback.as_str(),
                    streak,
                );
                push_pre_gate_user_feedback(p, feedback);
                per_coord.record_outer_loop_build_idle_feedback_injected();
                sync_per_flight_from_coord(p, per_coord);
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
                "outer loop pre-gate: {}",
                OuterLoopReflectPreGateReason::MissingFinalAnswerFeedback.as_str(),
            );
            push_pre_gate_user_feedback(p, feedback);
            per_coord.record_outer_loop_missing_final_answer_feedback_injected();
            sync_per_flight_from_coord(p, per_coord);
            return ReflectBranchCtl::ContinueOuter;
        }
    }
    sync_per_flight_from_coord(p, per_coord);
    ReflectBranchCtl::BreakOuter
}

async fn reflect_branch_on_plan_rewrite_exhausted(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    reason: &str,
) -> ReflectBranchCtl {
    sync_per_flight_from_coord(p, per_coord);
    if let Some(tx) = p.ctx.io.control.out {
        let _ = crate::sse::send_string_logged(
            tx,
            encode_message(SsePayload::Error(sse_plan_rewrite_exhausted_body(
                p.ctx.obs.tracing_chat_turn.as_ref(),
                reason,
            ))),
            "outer_loop::plan_rewrite_exhausted",
        )
        .await;
    }
    ReflectBranchCtl::BreakOuter
}

/// `per_reflect_after_assistant` 结果 → 外循环控制（含 build-idle 纠偏、plan_rewrite SSE 等）。
pub(super) async fn map_reflect_outcome_to_branch_ctl(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    msg: &Message,
    outcome: ReflectOnAssistantOutcome,
) -> ReflectBranchCtl {
    match outcome {
        ReflectOnAssistantOutcome::StopTurn => reflect_branch_on_stop_turn(p, per_coord, msg),
        ReflectOnAssistantOutcome::ContinueOuterForPlanRewrite => {
            if let Some(f) = p.ctx.attach.per_flight.as_ref() {
                f.sync_from_per_coord(per_coord);
                f.awaiting_plan_rewrite_model.store(true, Ordering::Relaxed);
            }
            ReflectBranchCtl::ContinueOuter
        }
        ReflectOnAssistantOutcome::ProceedToExecuteTools => {
            per_coord.reset_outer_loop_build_idle_streak();
            sync_per_flight_from_coord(p, per_coord);
            ReflectBranchCtl::ProceedToTools
        }
        ReflectOnAssistantOutcome::PlanRewriteExhausted { reason } => {
            reflect_branch_on_plan_rewrite_exhausted(p, per_coord, reason.as_str()).await
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
