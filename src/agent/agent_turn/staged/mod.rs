//! 分阶段规划与逻辑双 agent：规划轮 + 逐步注入执行。

use std::ops::ControlFlow;

use log::{debug, warn};

use crate::agent::per_coord::PerCoordinator;
use crate::agent::plan_artifact;
use crate::agent::plan_optimizer;
use crate::agent::reflection::plan_rewrite;
use crate::llm::no_tools_chat_request_from_messages;
use crate::types::{Message, USER_CANCELLED_FINISH_REASON};

use super::errors::{AgentTurnSubPhase, RunAgentTurnError};
use super::outer_loop::run_agent_outer_loop;
use super::params::RunLoopParams;

mod completed_replanning_suppression;
mod empty_execution;
mod ensemble_fsm;
mod ensemble_schedule_fsm;
mod full_pipeline_fsm;
mod orchestrator;
mod patch_planner;
mod plan_stagnation;
mod planner_parse_fsm;
mod planner_round_driver;
mod planner_round_fsm;
mod post_parse_pipeline_fsm;
mod prepared_parse_fsm;
mod prepared_post_parse_fsm;
mod rolling_horizon_facade;
mod sse;
mod staged_step_fsm;
mod step_after_outer;
mod step_failure_exit;
mod step_iteration_fsm;
mod step_loop_fsm;
mod steps_loop;
mod turn_fsm;

#[cfg(test)]
mod planner_tool_call_reject_regression_tests;
#[cfg(test)]
mod staged_plan_prepare_fixture_tests;

use sse as staged_sse;

use completed_replanning_suppression::should_suppress_completed_replanning;
use ensemble_fsm::{EnsembleMergeOutcome, ensemble_merge_outcome_from_parsed_steps};
use full_pipeline_fsm::{
    StagedFullPipelinePhase, debug_staged_full_pipeline_enter,
    debug_staged_full_pipeline_transition,
};
use patch_planner::StagedPlanPatchPlannerCtx;
use plan_stagnation::{
    StagedPlanStagnationAction, evaluate_staged_plan_stagnation_after_step_round,
};
use planner_parse_fsm::omit_no_task_planner_from_history;
use planner_round_driver::{
    complete_first_planner_round_maybe_retry_tool_reject,
    complete_one_staged_planner_assistant_round, emit_staged_planner_tool_call_rejected_timeline,
    maybe_run_staged_plan_ensemble_then_merge, run_staged_plan_nl_followup_round,
};
use post_parse_pipeline_fsm::{
    ensemble_merge_should_invoke, ensemble_merge_skip_for_casual_prompt,
    log_staged_plan_ensemble_route, log_staged_plan_optimizer_route, optimizer_round_should_run,
};
use prepared_parse_fsm::{PreparedPlannerRoute, resolve_prepared_planner_route};
use prepared_post_parse_fsm::{
    PreparedFullPipelineInputs, PreparedFullPipelineSchedule, PreparedPostParseSchedule,
    prepared_full_pipeline_schedule, prepared_post_parse_schedule,
};
use staged_sse::{next_staged_plan_id, staged_plan_phase_instruction_default};
use steps_loop::run_staged_plan_steps_loop;

// Re-export for `run_dispatch`, `agent_turn/tests`, and in-module `#[cfg(test)]`.
#[allow(unused_imports)]
pub(crate) use rolling_horizon_facade::{
    build_logical_dual_planner_messages, build_single_agent_planner_messages,
    run_logical_dual_agent_then_execute_steps, run_non_hierarchical_staged_route,
    run_staged_plan_then_execute_steps,
};

/// 单次无工具规划子调用结束时的粗粒度结果（滚动视界外层循环消费）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagedPlanRunOutcome {
    ContinuePlanning,
    Finished,
}

#[cfg(test)]
pub(crate) fn simulate_single_step_rolling_horizon_for_test(
    outcomes: &[StagedPlanRunOutcome],
    max_rounds: usize,
) -> Result<usize, String> {
    let mut staged_rounds = 0usize;
    let mut idx = 0usize;
    loop {
        staged_rounds = staged_rounds.saturating_add(1);
        if staged_rounds > max_rounds {
            return Err(format!(
                "分阶段单步规划轮次超过上限（{}），已停止以避免无限循环",
                max_rounds
            ));
        }
        let outcome = outcomes
            .get(idx)
            .copied()
            .unwrap_or(StagedPlanRunOutcome::ContinuePlanning);
        idx = idx.saturating_add(1);
        match outcome {
            StagedPlanRunOutcome::ContinuePlanning => continue,
            StagedPlanRunOutcome::Finished => return Ok(staged_rounds),
        }
    }
}

/// 分阶段规划共享执行路径上的日志文案（避免 `run_staged_plan_with_prepared_request` 参数过长）。
#[derive(Clone, Copy)]
pub(crate) struct StagedPlanRunLabels {
    pub planning_log_label: &'static str,
    pub step_injection_log_label: &'static str,
    pub build_planner_messages: fn(&[Message], String, bool, bool) -> Vec<Message>,
}

pub(super) async fn prepare_staged_planner_no_tools_request(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    build_planner_messages: fn(&[Message], String, bool, bool) -> Vec<Message>,
) -> Result<crate::types::ChatRequest, RunAgentTurnError> {
    if let Some(ref ltm) = p.ctx.attach.long_term_memory {
        ltm.prepare_messages(
            p.ctx.core.cfg.as_ref(),
            p.ctx.attach.long_term_memory_scope_id.as_deref(),
            p.turn.messages_buffer_mut(),
        );
    }
    p.prepare_turn_messages_for_model(Some(per_coord))
        .await
        .map_err(|e| RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: e.to_string(),
        })?;

    let immutable_appendix = p
        .turn
        .staged_immutable_user_goal_snapshot()
        .map(crate::agent::plan_optimizer::staged_rolling_immutable_plan_system_appendix);

    let instr = p
        .ctx
        .core
        .cfg
        .staged_planning
        .staged_plan_phase_instruction
        .trim();
    let mut plan_system = if instr.is_empty() {
        staged_plan_phase_instruction_default()
    } else {
        instr.to_string()
    };
    if let Some(app) = immutable_appendix {
        plan_system.push_str(&app);
    }
    let baseline_mode = p.ctx.core.cfg.staged_planning.staged_plan_baseline_mode;
    if baseline_mode != crate::config::StagedPlanBaselineMode::ImmutableGoalOnly
        && let Some(ref baseline) = p.turn.turn_planner_hints.staged_baseline_plan
    {
        plan_system.push_str(
            &plan_optimizer::staged_baseline_plan_planner_system_appendix(baseline, baseline_mode),
        );
    }
    let preserve_kimi = crate::llm::llm_vendor_adapter(p.ctx.core.cfg.as_ref())
        .preserve_assistant_tool_call_reasoning(p.ctx.core.cfg.as_ref());
    let preserve_deepseek =
        crate::llm::vendor::deepseek_json_output_eligible(p.ctx.core.cfg.as_ref());
    let mut req = no_tools_chat_request_from_messages(
        p.ctx.core.cfg.as_ref(),
        build_planner_messages(
            p.turn.messages(),
            plan_system,
            preserve_kimi,
            preserve_deepseek,
        ),
        p.turn.temperature_override,
        p.effective_model(),
        p.turn.seed_override,
    );
    req.max_tokens = req
        .max_tokens
        .max(crate::llm::STAGED_PLANNER_MIN_COMPLETION_TOKENS);
    Ok(req)
}

/// 首轮解析成功后 **`PreparedPlannerRoute::ContinueWithPlan`** 的后续管线（no_task / full-pipeline）参聚合。
struct ContinuePreparedPlanAfterFirstRoundParams<'a, 'b, F> {
    p: &'a mut RunLoopParams<'b>,
    per_coord: &'a mut PerCoordinator,
    labels: StagedPlanRunLabels,
    planner_render_to_terminal: bool,
    echo_terminal_staged: bool,
    entered_from_step_execution_round: bool,
    plan: plan_artifact::AgentReplyPlanV1,
    msg: Message,
    make_step_user_message: F,
}

async fn continue_prepared_plan_after_first_round<F>(
    params: ContinuePreparedPlanAfterFirstRoundParams<'_, '_, F>,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let ContinuePreparedPlanAfterFirstRoundParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        echo_terminal_staged,
        entered_from_step_execution_round,
        plan,
        msg,
        make_step_user_message,
    } = params;
    if should_suppress_completed_replanning(
        p,
        entered_from_step_execution_round,
        plan.steps.as_slice(),
    ) {
        debug!(
            target: "crabmate",
            "分阶段重规划：原始目标已有完成证据，且新计划仅包含重复验证/总结型步骤，直接结束"
        );
        return Ok(StagedPlanRunOutcome::Finished);
    }
    let omit_no_task_planner_from_history = omit_no_task_planner_from_history(
        p.ctx.io.out.is_some(),
        crate::web::web_ui_env::web_raw_assistant_output_env(),
        plan.no_task,
    );
    if !omit_no_task_planner_from_history {
        p.turn.push_assistant_merging_trailing_empty(msg.clone());
    }

    let post_schedule = prepared_post_parse_schedule(plan.no_task);
    tracing::debug!(
        target: "crabmate::staged",
        staged_fsm = "prepared_request",
        prepared_route = "continue_with_plan",
        post_parse_schedule = ?post_schedule,
        plan_no_task = plan.no_task,
        plan_steps_len = plan.steps.len(),
        sub_phase = "planner",
        "staged prepared_request continue: post-parse schedule"
    );

    match post_schedule {
        PreparedPostParseSchedule::NoTaskThenOuter => {
            run_no_task_branch_then_outer(p, per_coord, &make_step_user_message).await?;
            Ok(StagedPlanRunOutcome::Finished)
        }
        PreparedPostParseSchedule::FullPipelineThenSteps => {
            let parallel_csv = plan_optimizer::parallel_batchable_tool_names_csv_from_defs(
                p.ctx.core.tools_defs,
                &p.ctx.obs.process_handles.handler_lookup,
                p.ctx.core.cfg.as_ref(),
            );
            let validate_only_binding_active =
                plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages())
                    .is_some_and(|ids| !ids.is_empty());
            let trigger_user = plan_optimizer::staged_plan_trigger_user_content(p.turn.messages());
            let pipeline_schedule = prepared_full_pipeline_schedule(PreparedFullPipelineInputs {
                staged_plan_ensemble_count: p.ctx.attach.staged_plan_ensemble_count,
                staged_plan_skip_ensemble_on_casual_prompt: p
                    .ctx
                    .attach
                    .staged_plan_skip_ensemble_on_casual_prompt,
                validate_only_binding_active,
                trigger_user_content: trigger_user,
                plan_steps_len: plan.steps.len(),
                staged_plan_optimizer_round: p.ctx.attach.staged_plan_optimizer_round,
                staged_plan_optimizer_requires_parallel_tools: p
                    .ctx
                    .attach
                    .staged_plan_optimizer_requires_parallel_tools,
                parallel_tool_names_csv: parallel_csv.as_str(),
                staged_plan_two_phase_nl_display: p
                    .ctx
                    .core
                    .cfg
                    .staged_planning
                    .staged_plan_two_phase_nl_display,
            });

            advance_full_pipeline_phases_after_parse_inner(AdvanceFullPipelineAfterParseParams {
                p,
                per_coord,
                labels,
                planner_render_to_terminal,
                echo_terminal_staged,
                make_step_user_message: &make_step_user_message,
                plan,
                pipeline_schedule,
                parallel_csv,
            })
            .await
        }
    }
}

fn debug_first_planner_finish(labels: StagedPlanRunLabels, finish_reason: &str, msg: &Message) {
    debug!(
        target: "crabmate",
        "{} finish_reason={} assistant_preview={}",
        labels.planning_log_label,
        finish_reason,
        crate::redact::assistant_message_preview_for_log(msg)
    );
}

async fn strip_non_tool_planner_assistant_after_first_round(
    msg: &mut Message,
    p: &RunLoopParams<'_>,
) {
    let dsml_enabled = p
        .ctx
        .core
        .cfg
        .dsml_materialize
        .materialize_deepseek_dsml_tool_calls;
    let scan = crate::dsml::staged_no_tools_scan(msg, dsml_enabled, "·重写后");
    if scan.raw_native_count > 0 {
        warn!(
            target: "crabmate",
            "分阶段规划轮重写后仍返回 {} 条原生 tool_calls，严格无工具模式下将其忽略",
            scan.raw_native_count
        );
    }
    if scan.materialized_count > 0 {
        emit_staged_planner_tool_call_rejected_timeline(p.ctx.io.out, scan.materialized_count)
            .await;
        warn!(
            target: "crabmate",
            "分阶段规划轮重写后仍检测到 {} 条 DSML tool_calls；严格无工具模式下将其忽略",
            scan.materialized_count
        );
    }
}

async fn run_no_task_branch_then_outer<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    make_step_user_message: &F,
) -> Result<(), RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    if p.ctx
        .core
        .cfg
        .staged_planning
        .staged_plan_two_phase_nl_display
    {
        run_staged_plan_nl_followup_round(p, per_coord, make_step_user_message).await?;
    }
    debug!(
        target: "crabmate",
        "分阶段规划：no_task=true，跳过分步注入，转入常规对话循环"
    );
    run_agent_outer_loop(p, per_coord).await?;
    Ok(())
}

/// 分阶段规划优化轮：入参聚合（控制 `clippy::too_many_arguments`）。
struct StagedOptimizerRoundParams<'a, 'b, F> {
    p: &'a mut RunLoopParams<'b>,
    per_coord: &'a mut PerCoordinator,
    labels: StagedPlanRunLabels,
    planner_render_to_terminal: bool,
    make_step_user_message: &'a F,
    plan: &'a mut plan_artifact::AgentReplyPlanV1,
    optimizer_route: planner_round_fsm::StagedPlanOptimizerRoute,
    parallel_csv: &'a str,
}

async fn maybe_run_optimizer_round_and_apply_steps_inner<F>(
    params: StagedOptimizerRoundParams<'_, '_, F>,
) -> Result<ControlFlow<StagedPlanRunOutcome, ()>, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let StagedOptimizerRoundParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        make_step_user_message,
        plan,
        optimizer_route,
        parallel_csv,
    } = params;
    if !optimizer_round_should_run(optimizer_route) {
        return Ok(ControlFlow::Continue(()));
    }
    let opt_body = plan_optimizer::staged_plan_optimizer_user_body(plan, parallel_csv);
    p.turn.push_message(make_step_user_message(opt_body));
    let (mut opt_msg, opt_finish) = complete_one_staged_planner_assistant_round(
        p,
        per_coord,
        labels.build_planner_messages,
        planner_render_to_terminal,
        "分阶段规划优化轮模型输出",
    )
    .await?;
    if opt_finish == USER_CANCELLED_FINISH_REASON {
        p.turn.pop_last_staged_planner_coach_user_if_present();
        return Ok(ControlFlow::Break(StagedPlanRunOutcome::Finished));
    }
    crate::dsml::strip_staged_planner_message_tool_calls(
        &mut opt_msg,
        "优化轮",
        p.ctx
            .core
            .cfg
            .dsml_materialize
            .materialize_deepseek_dsml_tool_calls,
    );
    let opt_content = crate::types::message_content_as_str(&opt_msg.content).unwrap_or("");
    let merged_steps = plan_optimizer::try_parse_optimizer_reply(opt_content);
    match ensemble_merge_outcome_from_parsed_steps(merged_steps) {
        EnsembleMergeOutcome::AppliedSteps(steps) => {
            if steps.len() < plan.steps.len() {
                debug!(
                    target: "crabmate",
                    "分阶段规划优化轮：步数 {} -> {}",
                    plan.steps.len(),
                    steps.len()
                );
            }
            p.turn.push_assistant_merging_trailing_empty(opt_msg);
            plan.steps = steps;
        }
        EnsembleMergeOutcome::KeepPriorPlan => {
            warn!(
                target: "crabmate",
                "分阶段规划优化轮：未解析出合法 agent_reply_plan v1 或非空 steps，沿用首轮规划"
            );
            p.turn.pop_last_staged_planner_coach_user_if_present();
        }
    }
    Ok(ControlFlow::Continue(()))
}

/// 首轮解析后 full-pipeline 直至分步循环：入参聚合（控制 `clippy::too_many_arguments`）。
struct AdvanceFullPipelineAfterParseParams<'a, 'b, F> {
    p: &'a mut RunLoopParams<'b>,
    per_coord: &'a mut PerCoordinator,
    labels: StagedPlanRunLabels,
    planner_render_to_terminal: bool,
    echo_terminal_staged: bool,
    make_step_user_message: &'a F,
    plan: plan_artifact::AgentReplyPlanV1,
    pipeline_schedule: PreparedFullPipelineSchedule,
    parallel_csv: String,
}

async fn advance_full_pipeline_phases_after_parse_inner<F>(
    params: AdvanceFullPipelineAfterParseParams<'_, '_, F>,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let AdvanceFullPipelineAfterParseParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        echo_terminal_staged,
        make_step_user_message,
        mut plan,
        pipeline_schedule,
        parallel_csv,
    } = params;
    let ensemble_route = pipeline_schedule.ensemble_route;
    log_staged_plan_ensemble_route(ensemble_route, p.ctx.attach.staged_plan_ensemble_count);

    let mut fp_phase = StagedFullPipelinePhase::BeforeEnsemble;
    debug_staged_full_pipeline_enter(fp_phase);

    if ensemble_merge_should_invoke(ensemble_route) {
        let skip_ensemble_for_casual = ensemble_merge_skip_for_casual_prompt(ensemble_route);
        maybe_run_staged_plan_ensemble_then_merge(
            p,
            per_coord,
            &labels,
            make_step_user_message,
            planner_render_to_terminal,
            &mut plan,
            skip_ensemble_for_casual,
        )
        .await?;
    }
    let next_fp = fp_phase
        .advance()
        .expect("full_pipeline: before_ensemble -> after_ensemble");
    debug_staged_full_pipeline_transition(fp_phase, Some(next_fp));
    fp_phase = next_fp;

    let optimizer_route = pipeline_schedule.optimizer_route;
    log_staged_plan_optimizer_route(optimizer_route, plan.steps.len());

    match maybe_run_optimizer_round_and_apply_steps_inner(StagedOptimizerRoundParams {
        p,
        per_coord,
        labels,
        planner_render_to_terminal,
        make_step_user_message,
        plan: &mut plan,
        optimizer_route,
        parallel_csv: parallel_csv.as_str(),
    })
    .await?
    {
        ControlFlow::Break(outcome) => return Ok(outcome),
        ControlFlow::Continue(()) => {}
    }

    let next_fp = fp_phase
        .advance()
        .expect("full_pipeline: after_ensemble -> after_optimizer");
    debug_staged_full_pipeline_transition(fp_phase, Some(next_fp));
    fp_phase = next_fp;

    if pipeline_schedule.nl_followup_before_steps {
        run_staged_plan_nl_followup_round(p, per_coord, make_step_user_message).await?;
    }
    let next_fp = fp_phase
        .advance()
        .expect("full_pipeline: after_optimizer -> after_nl_followup");
    debug_staged_full_pipeline_transition(fp_phase, Some(next_fp));
    fp_phase = next_fp;

    debug_staged_full_pipeline_transition(fp_phase, None);

    let baseline_mode = p.ctx.core.cfg.staged_planning.staged_plan_baseline_mode;
    if baseline_mode != crate::config::StagedPlanBaselineMode::ImmutableGoalOnly
        && p.turn.turn_planner_hints.staged_baseline_plan.is_none()
    {
        p.turn.turn_planner_hints.staged_baseline_plan = Some(plan.clone());
        tracing::debug!(
            target: "crabmate::staged",
            staged_baseline_mode = ?baseline_mode,
            baseline_steps = plan.steps.len(),
            "staged: frozen baseline agent_reply_plan v1 before steps loop"
        );
    }

    let plan_id = next_staged_plan_id();
    let plan_steps = plan.steps;
    let original_steps = plan_steps.clone();
    let patch_ctx = StagedPlanPatchPlannerCtx {
        p,
        per_coord,
        labels: &labels,
        planner_render_to_terminal,
        make_step_user_message,
    };

    run_staged_plan_steps_loop(
        plan_id,
        plan_steps,
        original_steps,
        echo_terminal_staged,
        &labels,
        patch_ctx,
        make_step_user_message,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_staged_plan_with_prepared_request<F>(
    p: &mut RunLoopParams<'_>,
    per_coord: &mut PerCoordinator,
    mut req: crate::types::ChatRequest,
    render_to_terminal: bool,
    echo_terminal_staged: bool,
    entered_from_step_execution_round: bool,
    labels: StagedPlanRunLabels,
    make_step_user_message: F,
) -> Result<StagedPlanRunOutcome, RunAgentTurnError>
where
    F: Fn(String) -> Message,
{
    let planner_render_to_terminal = render_to_terminal
        && (p.ctx.io.out.is_some()
            || p.ctx
                .core
                .cfg
                .staged_planning
                .staged_plan_cli_show_planner_stream);
    loop {
        let (mut msg, finish_reason) = complete_first_planner_round_maybe_retry_tool_reject(
            p,
            per_coord,
            &req,
            planner_render_to_terminal,
            labels,
            &make_step_user_message,
        )
        .await?;

        debug_first_planner_finish(labels, finish_reason.as_str(), &msg);

        if finish_reason == USER_CANCELLED_FINISH_REASON {
            return Ok(StagedPlanRunOutcome::Finished);
        }

        strip_non_tool_planner_assistant_after_first_round(&mut msg, p).await;

        let merged_for_log =
            crate::agent::plan_artifact::assistant_merged_text_for_plan_artifact_parse(&msg);
        let validate_only_binding_ids =
            plan_rewrite::last_workflow_validate_binding_plan_node_ids(p.turn.messages());
        let parse_result =
        crate::agent::plan_artifact::parse_agent_reply_plan_v1_from_assistant_message_with_validate_only_binding_ids(
            &msg,
            validate_only_binding_ids.as_deref(),
        );
        let parse_err_detail = parse_result
            .as_ref()
            .err()
            .map(crate::agent::plan_artifact::plan_artifact_error_log_summary);
        let degrade_like_not_found = matches!(
            parse_result.as_ref().err(),
            Some(crate::agent::plan_artifact::PlanArtifactError::NotFound)
        );

        let user_task = p
            .turn
            .staged_immutable_user_goal_snapshot()
            .map(str::to_string);
        let route = resolve_prepared_planner_route(
            parse_result,
            entered_from_step_execution_round,
            &msg,
            merged_for_log,
            parse_err_detail,
            degrade_like_not_found,
            user_task.as_deref(),
        );
        tracing::debug!(
            target: "crabmate::staged",
            staged_fsm = "prepared_request",
            prepared_route = route.as_static_str(),
            entered_from_step_execution_round,
            sub_phase = "planner",
            "staged prepared_request first-round parse route"
        );

        match route {
            PreparedPlannerRoute::QuietFinish => {
                debug!(
                    target: "crabmate",
                    "分阶段重规划：检测到分步执行后重入且本轮未产出结构化计划，视为收敛完成，直接结束（避免重复总结）"
                );
                return Ok(StagedPlanRunOutcome::Finished);
            }
            PreparedPlannerRoute::FinishWithDirectPlannerAnswer => {
                p.turn.push_assistant_merging_trailing_empty(msg.clone());
                debug!(
                    target: "crabmate",
                    "分阶段规划：只读概览类实质终答已落盘，跳过外循环（避免重复回答）"
                );
                return Ok(StagedPlanRunOutcome::Finished);
            }
            PreparedPlannerRoute::DegradeToOuterLoop => {
                p.turn.push_assistant_merging_trailing_empty(msg.clone());
                run_agent_outer_loop(p, per_coord).await?;
                return Ok(StagedPlanRunOutcome::Finished);
            }
            PreparedPlannerRoute::ContinueWithPlan { plan } => {
                if let Some(action) = evaluate_staged_plan_stagnation_after_step_round(
                    p.turn.messages(),
                    &plan,
                    entered_from_step_execution_round,
                ) {
                    match action {
                        StagedPlanStagnationAction::StopAfterRepeatedPlan => {
                            warn!(
                                target: "crabmate::staged",
                                "staged plan stagnation: identical plan after step round exceeded auto-replan cap; finishing (step_count={})",
                                plan.steps.len(),
                            );
                            return Ok(StagedPlanRunOutcome::Finished);
                        }
                        StagedPlanStagnationAction::ReplanWithFeedback(user_body) => {
                            warn!(
                                target: "crabmate::staged",
                                "staged plan stagnation: injecting coach user and re-running planner (step_count={})",
                                plan.steps.len(),
                            );
                            p.turn.push_message(Message::user_only(user_body));
                            req = prepare_staged_planner_no_tools_request(
                                p,
                                per_coord,
                                labels.build_planner_messages,
                            )
                            .await?;
                            continue;
                        }
                    }
                }
                return continue_prepared_plan_after_first_round(
                    ContinuePreparedPlanAfterFirstRoundParams {
                        p,
                        per_coord,
                        labels,
                        planner_render_to_terminal,
                        echo_terminal_staged,
                        entered_from_step_execution_round,
                        plan,
                        msg,
                        make_step_user_message,
                    },
                )
                .await;
            }
        }
    }
}

#[cfg(test)]
mod staged_not_found_convergence_tests {
    use crate::agent::plan_artifact::PlanArtifactError;

    use super::planner_parse_fsm::{
        StagedPlannerParseRoute, entered_implies_finish_on_plan_not_found,
        staged_planner_parse_route,
    };

    #[test]
    fn not_found_does_not_quiet_finish_for_plain_first_round() {
        assert!(
            !entered_implies_finish_on_plan_not_found(false),
            "普通首轮（未进入步后重规划）遇到 NotFound 不应走 QuietFinishOnPlanNotFound"
        );
        assert!(!matches!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, false, "x", None),
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound
        ),);
    }

    #[test]
    fn not_found_finishes_only_after_step_execution_reentry() {
        assert!(
            entered_implies_finish_on_plan_not_found(true),
            "仅在同 turn 的步后重规划轮，NotFound 才应触发 QuietFinishOnPlanNotFound"
        );
        assert!(matches!(
            staged_planner_parse_route(&PlanArtifactError::NotFound, true, "x", None),
            StagedPlannerParseRoute::QuietFinishOnPlanNotFound
        ),);
    }
}
