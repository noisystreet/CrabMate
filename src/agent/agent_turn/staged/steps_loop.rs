//! 分阶段规划 **`steps` 执行循环**：步级 SSE、外层 `run_agent_outer_loop`、补丁恢复与队列摘要。

use std::collections::HashMap;
use std::sync::atomic::Ordering;

use log::debug;

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact::PlanStepV1;
use crate::tool_result::tool_message_content_ok_for_model;
use crate::types::{Message, staged_step_window_end_exclusive};

use super::super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::super::execute_tools::sse_sender_closed;
use super::super::outer_loop::run_agent_outer_loop;

use super::empty_execution::staged_step_empty_execution_verify_failure;
use super::orchestrator as staged_orchestrator;
use super::patch_planner::StagedPlanPatchPlannerCtx;
use super::sse::{
    StagedStepOkNoticeParams, emit_chat_ui_separator_sse, finish_staged_plan_step_sse,
    send_staged_plan_finished, send_staged_plan_step_started,
    staged_step_emit_ok_step_and_queue_notice,
};
use super::staged_step_fsm::{
    staged_patch_budget_after_step_failure, staged_patch_budget_tool_messages_not_ok,
    staged_step_patch_planner_enabled,
};
use super::staged_step_patch_recover::{
    StagedStepPatchRecoverBundles, StagedStepPatchRecoverSpec, staged_step_try_patch_recover,
};
use super::step_after_outer::{
    CfJumpMeta, CfJumpMut, staged_step_maybe_return_on_control_flow_jump,
};
use super::step_failure_exit::{
    StagedPlanStepFailedExit, StagedStepOuterFailureExitParams,
    finish_staged_plan_step_failed_and_plan_failed_sse,
    staged_step_fail_after_outer_execution_exhausted,
};
use super::step_iteration_fsm::{StagedStepIterationCtl, staged_step_wall_clock_exceeded};
use super::step_loop_fsm::staged_injected_step_user_body;
use super::step_patch_route_fsm::{
    StagedStepPatchFailureKind, resolve_staged_step_patch_failure_kind,
};
use super::steps_loop_route_fsm::{
    StagedStepPostOuterRoute, resolve_staged_step_post_outer_route_from_results,
};
use super::turn_orchestrator_fsm::{
    orchestrator_phase_for_round_orchestrator, orchestrator_phase_for_steps_loop_trace,
};
use super::{StagedPlanRunLabels, StagedPlanRunOutcome};

/// 自本步 user 注入起至步界（或历史末尾）之间的 `role: tool` 是否均为成功（与信封 `ok` / 传统解析一致）。
fn staged_step_tool_messages_all_ok(messages: &[Message], step_user_index: usize) -> bool {
    let end = staged_step_window_end_exclusive(messages, step_user_index);
    let mut i = step_user_index.saturating_add(1);
    while i < end {
        let m = &messages[i];
        if m.role == "tool" {
            let content = crate::types::message_content_as_str(&m.content).unwrap_or("");
            if !tool_message_content_ok_for_model(content, "") {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// outer_loop 与验收之后、transition / 补丁 / 工具检查 / 成功收尾 之前的数据（**AfterOuterLoop** 阶段入参）。
struct StagedStepOuterHalfResult {
    step: PlanStepV1,
    step_index: usize,
    step_user_idx: usize,
    run_step: Result<(), RunAgentTurnError>,
    step_verify_failed_reason: Option<String>,
}

struct StagedStepRunOuterHalfParams<'a, 'b, 'c, F> {
    plan_id: &'a str,
    i: usize,
    n: usize,
    plan_steps: &'a [PlanStepV1],
    echo_terminal_staged: bool,
    labels: &'a StagedPlanRunLabels,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    make_step_user_message: &'a F,
}

/// **`StagedStepRunningSub::BeforeStepLlm`** → **`InOuterLoop`**：发 `step_started`、注入 user、`run_agent_outer_loop`、可选 acceptance。
async fn staged_step_run_outer_half<F>(
    p: StagedStepRunOuterHalfParams<'_, '_, '_, F>,
) -> StagedStepOuterHalfResult
where
    F: Fn(String) -> Message,
{
    let StagedStepRunOuterHalfParams {
        plan_id,
        i,
        n,
        plan_steps,
        echo_terminal_staged,
        labels,
        patch_ctx,
        make_step_user_message,
    } = p;
    let step = plan_steps[i].clone();
    let step_index = i + 1;
    send_staged_plan_step_started(
        patch_ctx.p.ctx.io.out,
        plan_id,
        step.id.trim(),
        step_index,
        n,
        step.description.trim(),
        step.executor_kind.map(|k| k.as_snake_case_str()),
    )
    .await;

    let immutable = patch_ctx.p.turn.staged_immutable_user_goal_snapshot();
    let body = staged_injected_step_user_body(step_index, n, &step, immutable);
    debug!(
        target: "crabmate",
        "{} step={}/{} body_len={} body_preview={}",
        labels.step_injection_log_label,
        i + 1,
        n,
        body.len(),
        crate::redact::preview_chars(&body, crate::redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    if echo_terminal_staged {
        let _ = crate::runtime::terminal_cli_transcript::print_staged_plan_notice(false, &body);
    }
    let step_user_idx = patch_ctx.p.turn.messages().len();
    patch_ctx.p.turn.push_message(make_step_user_message(body));
    let prev_executor_constraint = patch_ctx.p.turn.turn_planner_hints.step_executor_constraint;
    patch_ctx.p.turn.turn_planner_hints.step_executor_constraint = step.executor_kind;
    let run_step = run_agent_outer_loop(patch_ctx.p, patch_ctx.per_coord).await;
    patch_ctx.p.turn.turn_planner_hints.step_executor_constraint = prev_executor_constraint;

    let mut step_verify_failed_reason: Option<String> = None;
    if run_step.is_ok() {
        step_verify_failed_reason = staged_step_empty_execution_verify_failure(
            patch_ctx.p.turn.messages(),
            step_user_idx,
            &step,
            patch_ctx.p.ctx.core.effective_working_dir,
        );
        if step_verify_failed_reason.is_none() {
            #[allow(clippy::collapsible_if)]
            if let Some(acceptance) =
                crate::agent::acceptance::effective_plan_step_acceptance(&step)
            {
                let verify_result = crate::agent::step_verifier::verify_step_execution(
                    &acceptance,
                    patch_ctx.p.turn.messages(),
                    step_user_idx,
                    patch_ctx.p.ctx.core.effective_working_dir,
                );

                if let crate::agent::step_verifier::VerifyResult::Fail { reason } = verify_result {
                    step_verify_failed_reason = Some(reason);
                }
            }
        }
    }

    staged_step_clear_run_command_cache_on_verify_failure(
        patch_ctx.per_coord,
        &step_verify_failed_reason,
    );

    StagedStepOuterHalfResult {
        step,
        step_index,
        step_user_idx,
        run_step,
        step_verify_failed_reason,
    }
}

fn staged_step_clear_run_command_cache_on_verify_failure(
    per_coord: &mut PerCoordinator,
    step_verify_failed_reason: &Option<String>,
) {
    if step_verify_failed_reason.is_some() {
        per_coord.clear_all_run_command_failure_state();
    }
}

/// **`StagedStepRunningSub::AfterOuterLoop`**：transition、失败补丁、取消、工具补丁、成功 SSE。
struct StagedStepRunAfterOuterHalfParams<'a, 'b, 'c, F> {
    outer: StagedStepOuterHalfResult,
    plan_id: &'a str,
    i: usize,
    n: usize,
    completed_steps: usize,
    plan_steps: &'a mut Vec<PlanStepV1>,
    original_steps: &'a [PlanStepV1],
    transition_counters: &'a mut HashMap<String, u32>,
    echo_terminal_staged: bool,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
}

/// 外层循环失败后的补丁恢复（降低 `staged_step_run_after_outer_half` 圈复杂度）。
struct StagedOuterExecFailureRecoverParams<'a, 'b, 'c, F> {
    plan_id: &'a str,
    i: usize,
    n: usize,
    completed_steps: usize,
    plan_steps: &'a mut Vec<PlanStepV1>,
    echo_terminal_staged: bool,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    step: &'a PlanStepV1,
    step_user_index: usize,
    step_verify_failed_reason: &'a Option<String>,
    /// `run_agent_outer_loop` 返回 `Err` 时的 `to_string()`，供补丁规划 user 闭环反馈。
    outer_loop_error_text: Option<String>,
}

async fn staged_step_try_recover_outer_execution_failure<F>(
    p: StagedOuterExecFailureRecoverParams<'_, '_, '_, F>,
) -> Result<Option<StagedStepIterationCtl>, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedOuterExecFailureRecoverParams {
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        echo_terminal_staged,
        patch_ctx,
        step,
        step_user_index,
        step_verify_failed_reason,
        outer_loop_error_text,
    } = p;
    if !staged_step_patch_planner_enabled(
        patch_ctx
            .p
            .ctx
            .core
            .cfg
            .staged_planning
            .staged_plan_feedback_mode,
    ) {
        return Ok(None);
    }
    let failure_kind = resolve_staged_step_patch_failure_kind(
        step_verify_failed_reason,
        outer_loop_error_text.is_some(),
    )
    .unwrap_or(StagedStepPatchFailureKind::OuterLoopError);
    let patch_budget = staged_patch_budget_after_step_failure(
        step.max_step_retries,
        patch_ctx
            .p
            .ctx
            .core
            .cfg
            .staged_planning
            .staged_plan_patch_max_attempts,
    );
    staged_step_try_patch_recover(
        StagedStepPatchRecoverBundles {
            plan_id,
            i,
            n,
            completed_steps,
            plan_steps,
            echo_terminal_staged,
            patch_ctx,
            step,
            step_user_index,
        },
        StagedStepPatchRecoverSpec {
            failure_kind,
            steps_loop_phase: "patch_replanner_attempt",
            patch_budget,
            outer_loop_error_text: outer_loop_error_text.as_deref(),
        },
    )
    .await
}

/// 工具消息未全部成功时的补丁恢复：`Ok(Some(RetryCurrentStep))` 或 `Ok(None)`（由调用方走工具失败耗尽）。
struct StagedToolFailurePatchRecoverParams<'a, 'b, 'c, F> {
    plan_id: &'a str,
    i: usize,
    n: usize,
    completed_steps: usize,
    plan_steps: &'a mut Vec<PlanStepV1>,
    echo_terminal_staged: bool,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    step_user_index: usize,
    step: &'a PlanStepV1,
}

async fn staged_step_try_recover_tool_failure_patches<F>(
    p: StagedToolFailurePatchRecoverParams<'_, '_, '_, F>,
) -> Result<Option<StagedStepIterationCtl>, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedToolFailurePatchRecoverParams {
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        echo_terminal_staged,
        patch_ctx,
        step_user_index,
        step,
    } = p;
    let tool_patch_budget = staged_patch_budget_tool_messages_not_ok(
        patch_ctx
            .p
            .ctx
            .core
            .cfg
            .staged_planning
            .staged_plan_patch_max_attempts,
    );
    staged_step_try_patch_recover(
        StagedStepPatchRecoverBundles {
            plan_id,
            i,
            n,
            completed_steps,
            plan_steps,
            echo_terminal_staged,
            patch_ctx,
            step,
            step_user_index,
        },
        StagedStepPatchRecoverSpec {
            failure_kind: StagedStepPatchFailureKind::ToolMessagesNotOk,
            steps_loop_phase: "patch_replanner_tool_failure",
            patch_budget: tool_patch_budget,
            outer_loop_error_text: None,
        },
    )
    .await
}

async fn staged_step_run_after_outer_half<F>(
    p: StagedStepRunAfterOuterHalfParams<'_, '_, '_, F>,
) -> Result<StagedStepIterationCtl, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedStepRunAfterOuterHalfParams {
        outer,
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        original_steps,
        transition_counters,
        echo_terminal_staged,
        patch_ctx,
    } = p;
    let StagedStepOuterHalfResult {
        step,
        step_index,
        step_user_idx,
        run_step,
        step_verify_failed_reason,
    } = outer;

    let out = patch_ctx.p.ctx.io.out;

    if let Some(ctl) = staged_step_maybe_return_on_control_flow_jump(
        CfJumpMut {
            patch_ctx,
            plan_steps,
            transition_counters,
        },
        &step,
        CfJumpMeta {
            original_steps,
            step_loop_index: i,
            step_display_index: step_index,
            completed_steps,
            run_step: &run_step,
            step_verify_failed_reason: &step_verify_failed_reason,
            out,
            plan_id,
            echo_terminal_staged,
        },
    )
    .await
    {
        return Ok(ctl);
    }

    let cancelled = sse_sender_closed(out)
        || patch_ctx
            .p
            .ctx
            .io
            .cancel
            .is_some_and(|c| c.load(Ordering::SeqCst));
    let tools_ok = staged_step_tool_messages_all_ok(patch_ctx.p.turn.messages(), step_user_idx);
    let patch_planner_on = staged_step_patch_planner_enabled(
        patch_ctx
            .p
            .ctx
            .core
            .cfg
            .staged_planning
            .staged_plan_feedback_mode,
    );
    let post_outer_route = resolve_staged_step_post_outer_route_from_results(
        &run_step,
        &step_verify_failed_reason,
        cancelled,
        tools_ok,
        patch_planner_on,
    );
    tracing::debug!(
        target: "crabmate::staged",
        staged_fsm = "steps_loop",
        steps_loop_phase = "post_outer_route",
        steps_loop_post_outer_route = post_outer_route.as_str(),
        sub_phase = "executor",
        "staged step after outer_loop: route resolved"
    );

    match post_outer_route {
        StagedStepPostOuterRoute::ExecOrVerifyFailed => {
            if let Some(ctl) = staged_step_try_recover_outer_execution_failure(
                StagedOuterExecFailureRecoverParams {
                    plan_id,
                    i,
                    n,
                    completed_steps,
                    plan_steps,
                    echo_terminal_staged,
                    patch_ctx,
                    step: &step,
                    step_user_index: step_user_idx,
                    step_verify_failed_reason: &step_verify_failed_reason,
                    outer_loop_error_text: run_step.as_ref().err().map(|e| e.to_string()),
                },
            )
            .await?
            {
                return Ok(ctl);
            }
            return staged_step_fail_after_outer_execution_exhausted(
                StagedStepOuterFailureExitParams {
                    out,
                    plan_id,
                    step: &step,
                    step_index,
                    step_user_idx,
                    n,
                    completed_steps,
                    run_step: &run_step,
                    step_verify_failed_reason: &step_verify_failed_reason,
                    patch_ctx,
                },
            )
            .await;
        }
        StagedStepPostOuterRoute::Cancelled => {
            finish_staged_plan_step_sse(
                out,
                plan_id,
                step.id.trim(),
                step_index,
                n,
                "cancelled",
                step.executor_kind,
                None,
            )
            .await;
            return Ok(StagedStepIterationCtl::CancelledAfterOuterOk);
        }
        StagedStepPostOuterRoute::ToolFailurePatch => {
            if let Some(ctl) =
                staged_step_try_recover_tool_failure_patches(StagedToolFailurePatchRecoverParams {
                    plan_id,
                    i,
                    n,
                    completed_steps,
                    plan_steps,
                    echo_terminal_staged,
                    patch_ctx,
                    step_user_index: step_user_idx,
                    step: &step,
                })
                .await?
            {
                return Ok(ctl);
            }
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
                None,
            )
            .await;
            return Err(RunAgentTurnError::StepRetryExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: format!(
                    "局部修复耗尽上限 (工具执行失败){}",
                    patch_ctx
                        .per_coord
                        .staged_plan_patch_vs_plan_rewrite_counters_footer()
                ),
            });
        }
        StagedStepPostOuterRoute::EmitSuccess => {}
    }

    staged_step_emit_ok_step_and_queue_notice(StagedStepOkNoticeParams {
        out,
        messages: patch_ctx.p.turn.messages_buffer_mut(),
        plan_id,
        step_id_trim: step.id.trim(),
        step_index,
        n,
        executor_kind: step.executor_kind,
        plan_steps: plan_steps.as_slice(),
        echo_terminal_staged,
    })
    .await;
    Ok(StagedStepIterationCtl::AdvanceToNextStep {
        n,
        completed_steps: step_index,
    })
}

struct RunOneStagedPlanStepIterationParams<'a, 'b, 'c, F> {
    plan_id: &'a str,
    i: usize,
    n: usize,
    completed_steps: usize,
    plan_steps: &'a mut Vec<PlanStepV1>,
    original_steps: &'a [PlanStepV1],
    transition_counters: &'a mut HashMap<String, u32>,
    echo_terminal_staged: bool,
    labels: &'a StagedPlanRunLabels,
    patch_ctx: &'a mut StagedPlanPatchPlannerCtx<'b, 'c, F>,
    make_step_user_message: &'a F,
}

async fn run_one_staged_plan_step_iteration<F>(
    p: RunOneStagedPlanStepIterationParams<'_, '_, '_, F>,
) -> Result<StagedStepIterationCtl, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let RunOneStagedPlanStepIterationParams {
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        original_steps,
        transition_counters,
        echo_terminal_staged,
        labels,
        patch_ctx,
        make_step_user_message,
    } = p;
    let outer = staged_step_run_outer_half(StagedStepRunOuterHalfParams {
        plan_id,
        i,
        n,
        plan_steps: plan_steps.as_slice(),
        echo_terminal_staged,
        labels,
        patch_ctx,
        make_step_user_message,
    })
    .await;

    staged_step_run_after_outer_half(StagedStepRunAfterOuterHalfParams {
        outer,
        plan_id,
        i,
        n,
        completed_steps,
        plan_steps,
        original_steps,
        transition_counters,
        echo_terminal_staged,
        patch_ctx,
    })
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_staged_plan_steps_loop<F>(
    plan_id: String,
    mut plan_steps: Vec<PlanStepV1>,
    original_steps: Vec<PlanStepV1>,
    echo_terminal_staged: bool,
    labels: &StagedPlanRunLabels,
    mut patch_ctx: StagedPlanPatchPlannerCtx<'_, '_, F>,
    make_step_user_message: &F,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let mut n = plan_steps.len();
    let orch_phase = staged_orchestrator::enter_steps_executing(
        patch_ctx.p.ctx.io.out,
        plan_id.as_str(),
        echo_terminal_staged,
        plan_steps.as_slice(),
    )
    .await;
    tracing::info!(
        target: "crabmate::staged",
        staged_fsm = "steps_loop",
        steps_loop_phase = "steps_executing_enter",
        staged_turn_orchestrator_phase = orchestrator_phase_for_round_orchestrator(orch_phase)
            .as_str(),
        staged_round_orchestrator_phase = ?orch_phase,
        plan_id = plan_id.as_str(),
        step_count = n,
        sub_phase = "executor",
        "staged plan steps loop: started SSE + queue notice"
    );

    let mut staged_loop_cancelled = false;
    let mut completed_steps = 0usize;
    let mut i = 0usize;
    let mut transition_counters: HashMap<String, u32> = HashMap::new();
    let start_time = std::time::Instant::now();
    while i < plan_steps.len() {
        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "steps_loop",
            steps_loop_phase = "step_running",
            staged_turn_orchestrator_phase =
                orchestrator_phase_for_steps_loop_trace("step_running").as_str(),
            plan_id = plan_id.as_str(),
            step_index = i,
            step_count = n,
            completed_steps,
            sub_phase = "executor",
            "staged plan steps loop iteration enter"
        );
        let max_turn_s = patch_ctx
            .p
            .ctx
            .core
            .cfg
            .turn_budget
            .max_turn_duration_seconds;
        if staged_step_wall_clock_exceeded(max_turn_s, start_time.elapsed().as_secs()) {
            return Err(RunAgentTurnError::TimeLimitExhausted {
                phase: AgentTurnSubPhase::Executor,
                message: crate::agent::turn_budget::turn_wall_clock_limit_user_message(max_turn_s),
            });
        }

        if sse_sender_closed(patch_ctx.p.ctx.io.out)
            || patch_ctx
                .p
                .ctx
                .io
                .cancel
                .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            staged_loop_cancelled = true;
            tracing::info!(
                target: "crabmate::staged",
                staged_fsm = "steps_loop",
                steps_loop_phase = "cancelled_before_step",
                plan_id = plan_id.as_str(),
                step_index = i,
                step_count = n,
                completed_steps,
                sub_phase = "executor",
                "staged plan steps loop: SSE closed or user cancel"
            );
            break;
        }

        match run_one_staged_plan_step_iteration(RunOneStagedPlanStepIterationParams {
            plan_id: plan_id.as_str(),
            i,
            n,
            completed_steps,
            plan_steps: &mut plan_steps,
            original_steps: original_steps.as_slice(),
            transition_counters: &mut transition_counters,
            echo_terminal_staged,
            labels,
            patch_ctx: &mut patch_ctx,
            make_step_user_message,
        })
        .await?
        {
            StagedStepIterationCtl::RetryCurrentStep { n: new_n } => {
                n = new_n;
            }
            StagedStepIterationCtl::AdvanceToNextStep {
                n: new_n,
                completed_steps: new_completed,
            } => {
                n = new_n;
                completed_steps = new_completed;
                i += 1;
            }
            StagedStepIterationCtl::CancelledAfterOuterOk => {
                staged_loop_cancelled = true;
                tracing::info!(
                    target: "crabmate::staged",
                    staged_fsm = "steps_loop",
                    steps_loop_phase = "cancelled_after_outer_ok",
                    plan_id = plan_id.as_str(),
                    step_index = i,
                    step_count = n,
                    completed_steps,
                    sub_phase = "executor",
                    "staged plan steps loop: cancelled after outer_loop ok"
                );
                break;
            }
        }
    }
    tracing::info!(
        target: "crabmate::staged",
        staged_fsm = "steps_loop",
        steps_loop_phase = "send_plan_finished",
        plan_id = plan_id.as_str(),
        step_count = n,
        completed_steps,
        finish_status = if staged_loop_cancelled {
            "cancelled"
        } else {
            "ok"
        },
        sub_phase = "executor",
        "staged plan steps loop: emitting staged_plan_finished"
    );
    // 末步成功后循环内已发送含「[✓] 全部完成」的摘要，勿再发一次（否则重复一条）。
    send_staged_plan_finished(
        patch_ctx.p.ctx.io.out,
        &plan_id,
        n,
        completed_steps,
        if staged_loop_cancelled {
            "cancelled"
        } else {
            "ok"
        },
    )
    .await;
    // 仅当循环内未添加过分隔符时再追加：n==0 未进入循环；或取消时 completed_steps==0。
    // 否则末步成功后已在循环内添加，再加会重复两行。
    if n == 0 || (staged_loop_cancelled && completed_steps == 0) {
        patch_ctx
            .p
            .turn
            .push_message(Message::chat_ui_separator(true));
        emit_chat_ui_separator_sse(patch_ctx.p.ctx.io.out, true).await;
    }
    Ok(StagedPlanRunOutcome::ContinuePlanning)
}
