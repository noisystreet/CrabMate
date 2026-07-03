//! PER 编排 FSM 金样回归：`fixtures/fsm_orchestrator_golden.jsonl`。

use super::completed_replanning_reduce::reduce_completed_replanning_suppression;
use super::full_pipeline_fsm::StagedFullPipelinePhase;
use super::full_pipeline_reduce::{FullPipelineSegment, reduce_full_pipeline_segment};
use super::prepared_parse_fsm::PreparedPlannerRoute;
use super::prepared_post_parse_fsm::{PreparedFullPipelineSchedule, PreparedPostParseSchedule};
use super::prepared_route_reduce::reduce_prepared_planner_route;
use super::prepared_stagnation_reduce::reduce_prepared_stagnation_after_parse;
use super::rolling_horizon_advance_reduce::reduce_rolling_horizon_advance;
use super::rolling_horizon_preflight_reduce::{
    RollingHorizonPreflightInput, reduce_rolling_horizon_preflight,
};
use super::step_iteration_fsm::StagedStepIterationCtl;
use super::step_iteration_reduce::reduce_staged_step_post_outer_route;
use super::step_patch_recover_reduce::{
    StepPatchRecoverBranch, StepPatchRecoverReduceAction, StepPatchRecoverReduceInput,
    reduce_step_patch_recover,
};
use super::step_patch_route_fsm::{
    StagedStepPatchFailureKind, resolve_staged_step_patch_failure_kind,
};
use super::steps_loop_reduce::{reduce_steps_loop_iteration_ctl, reduce_steps_loop_preflight};
use super::steps_loop_route_fsm::{
    StagedStepPostOuterRoute, resolve_staged_step_post_outer_route_from_results,
};
use super::turn_fsm::{StagedTurnAdvance, StagedTurnPhase};
use super::turn_orchestrator_fsm::{
    orchestrator_phase_for_full_pipeline, orchestrator_phase_for_post_parse_schedule,
    orchestrator_phase_for_prepared_route, orchestrator_phase_for_prepared_route_reduce,
    orchestrator_phase_for_rolling_horizon_preflight, orchestrator_phase_for_step_iteration_reduce,
    orchestrator_phase_for_steps_loop_trace, orchestrator_phase_for_turn_phase,
};
use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};
use crate::agent::agent_turn::outer_loop_fsm::{OuterLoopIterationExit, ReflectBranchCtl};
use crate::agent::agent_turn::outer_loop_iteration_reduce::{
    outer_loop_iteration_exit_from_reflect_reduce, reduce_outer_loop_post_tools_exit,
    reduce_outer_loop_reflect_branch,
};
use crate::agent::agent_turn::staged::planner_round_fsm::{
    StagedPlanEnsembleRoute, StagedPlanOptimizerRoute,
};
use crate::agent::plan_artifact::AgentReplyPlanV1;
use crate::config::StagedPlanFeedbackMode;
use crate::types::Message;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    case: String,
    #[serde(flatten)]
    body: serde_json::Value,
}

fn line_ctx(path: &Path, line_no: usize, id: &str) -> String {
    format!("{}:{} ({})", path.display(), line_no + 1, id)
}

fn body_str<'a>(body: &'a serde_json::Value, key: &str, ctx: &str) -> &'a str {
    body[key]
        .as_str()
        .unwrap_or_else(|| panic!("{ctx}: missing {key}"))
}

fn prepared_route_from_label(label: &str) -> PreparedPlannerRoute {
    match label {
        "quiet_finish" => PreparedPlannerRoute::QuietFinish,
        "degrade_to_outer_loop" => PreparedPlannerRoute::DegradeToOuterLoop,
        "finish_with_direct_planner_answer" => PreparedPlannerRoute::FinishWithDirectPlannerAnswer,
        "continue_with_plan" => PreparedPlannerRoute::ContinueWithPlan {
            plan: AgentReplyPlanV1 {
                plan_type: "agent_reply_plan".into(),
                version: 1,
                steps: vec![],
                no_task: false,
            },
        },
        other => panic!("unknown prepared route label: {other}"),
    }
}

fn turn_phase_from_label(label: &str) -> StagedTurnPhase {
    match label {
        "pre_step_execution_round" => StagedTurnPhase::PreStepExecutionRound,
        "after_step_execution_round" => StagedTurnPhase::AfterStepExecutionRound,
        other => panic!("unknown turn phase label: {other}"),
    }
}

fn full_pipeline_phase_from_label(label: &str) -> StagedFullPipelinePhase {
    match label {
        "before_ensemble" => StagedFullPipelinePhase::BeforeEnsemble,
        "after_ensemble" => StagedFullPipelinePhase::AfterEnsemble,
        "after_optimizer" => StagedFullPipelinePhase::AfterOptimizer,
        "after_nl_followup" => StagedFullPipelinePhase::AfterNlFollowup,
        other => panic!("unknown full pipeline phase label: {other}"),
    }
}

fn reflect_ctl_from_label(label: &str) -> ReflectBranchCtl {
    match label {
        "break_outer" => ReflectBranchCtl::BreakOuter,
        "continue_outer" => ReflectBranchCtl::ContinueOuter,
        "proceed_to_tools" => ReflectBranchCtl::ProceedToTools,
        other => panic!("unknown reflect ctl label: {other}"),
    }
}

fn outer_exit_from_label(label: &str) -> OuterLoopIterationExit {
    match label {
        "stop_outer_loop" => OuterLoopIterationExit::StopOuterLoop,
        "continue_next_iteration" => OuterLoopIterationExit::ContinueNextIteration,
        other => panic!("unknown outer loop exit label: {other}"),
    }
}

fn assert_orchestrator_prepared_route(ctx: &str, body: &serde_json::Value) {
    let route = body_str(body, "route", ctx);
    let expect = body_str(body, "expect_phase", ctx);
    let got = orchestrator_phase_for_prepared_route(&prepared_route_from_label(route));
    assert_eq!(got.as_str(), expect, "{ctx}: prepared route");
}

fn assert_orchestrator_turn_phase(ctx: &str, body: &serde_json::Value) {
    let phase = body_str(body, "phase", ctx);
    let expect = body_str(body, "expect_phase", ctx);
    let got = orchestrator_phase_for_turn_phase(turn_phase_from_label(phase));
    assert_eq!(got.as_str(), expect, "{ctx}: turn phase");
}

fn assert_orchestrator_full_pipeline(ctx: &str, body: &serde_json::Value) {
    let phase = body_str(body, "phase", ctx);
    let expect = body_str(body, "expect_phase", ctx);
    let got = orchestrator_phase_for_full_pipeline(full_pipeline_phase_from_label(phase));
    assert_eq!(got.as_str(), expect, "{ctx}: full pipeline");
}

fn assert_orchestrator_steps_loop_trace(ctx: &str, body: &serde_json::Value) {
    let trace = body_str(body, "trace", ctx);
    let expect = body_str(body, "expect_phase", ctx);
    let got = orchestrator_phase_for_steps_loop_trace(trace);
    assert_eq!(got.as_str(), expect, "{ctx}: steps loop trace");
}

fn assert_outer_loop_reflect_ctl(ctx: &str, body: &serde_json::Value) {
    let ctl = body_str(body, "ctl", ctx);
    let expect = body_str(body, "expect", ctx);
    assert_eq!(reflect_ctl_from_label(ctl).as_trace_str(), expect, "{ctx}");
}

fn assert_outer_loop_iteration_exit(ctx: &str, body: &serde_json::Value) {
    let exit = body_str(body, "exit", ctx);
    let expect = body_str(body, "expect", ctx);
    assert_eq!(outer_exit_from_label(exit).as_trace_str(), expect, "{ctx}");
}

fn advance_from_body(body: &serde_json::Value, ctx: &str) -> StagedTurnAdvance {
    let label = body_str(body, "advance", ctx);
    match label {
        "continue" => {
            let phase = turn_phase_from_label(body_str(body, "phase", ctx));
            let has_feedback = body["has_feedback"].as_bool().unwrap_or(false);
            StagedTurnAdvance::Continue {
                phase,
                push_feedback_user: has_feedback.then(|| Message::user_only("fb")),
            }
        }
        "finished" => StagedTurnAdvance::Finished,
        "replan_exhausted" => StagedTurnAdvance::ReplanExhausted {
            phase: AgentTurnSubPhase::Planner,
            message: "exhausted".into(),
        },
        "propagate" => StagedTurnAdvance::Propagate(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Planner,
            message: "x".into(),
        }),
        other => panic!("{ctx}: unknown advance label {other}"),
    }
}

fn assert_rolling_horizon_advance_reduce(ctx: &str, body: &serde_json::Value) {
    let advance = advance_from_body(body, ctx);
    let expect = body_str(body, "expect", ctx);
    let action = reduce_rolling_horizon_advance(&advance);
    assert_eq!(
        action.as_str(),
        expect,
        "{ctx}: rolling horizon advance reduce"
    );
}

fn assert_outer_loop_reflect_reduce(ctx: &str, body: &serde_json::Value) {
    let ctl = reflect_ctl_from_label(body_str(body, "ctl", ctx));
    let expect = body_str(body, "expect", ctx);
    let action = reduce_outer_loop_reflect_branch(ctl);
    assert_eq!(action.as_str(), expect, "{ctx}: outer loop reflect reduce");
    if let Some(expect_exit) = body.get("expect_exit").and_then(|v| v.as_str()) {
        let exit = outer_loop_iteration_exit_from_reflect_reduce(action)
            .expect("{ctx}: expected Some exit");
        assert_eq!(
            exit.as_trace_str(),
            expect_exit,
            "{ctx}: reflect reduce exit"
        );
    } else if body.get("expect_exit").is_some() {
        assert!(
            outer_loop_iteration_exit_from_reflect_reduce(action).is_none(),
            "{ctx}: expected no iteration exit"
        );
    }
}

fn assert_outer_loop_post_tools_reduce(ctx: &str, body: &serde_json::Value) {
    let early_stop = body["early_stop"].as_bool().unwrap_or(false);
    let expect = body_str(body, "expect", ctx);
    let exit = reduce_outer_loop_post_tools_exit(early_stop);
    assert_eq!(
        exit.as_trace_str(),
        expect,
        "{ctx}: outer loop post tools reduce"
    );
}

fn assert_steps_loop_preflight_reduce(ctx: &str, body: &serde_json::Value) {
    let sse_closed = body["sse_closed"].as_bool().unwrap_or(false);
    let user_cancelled = body["user_cancelled"].as_bool().unwrap_or(false);
    let expect = body_str(body, "expect", ctx);
    let action = reduce_steps_loop_preflight(sse_closed, user_cancelled);
    assert_eq!(
        match action {
            super::steps_loop_reduce::StepsLoopPreflightReduceAction::Continue => {
                "continue"
            }
            super::steps_loop_reduce::StepsLoopPreflightReduceAction::BreakCancelled => {
                "break_cancelled"
            }
        },
        expect,
        "{ctx}: steps loop preflight"
    );
}

fn assert_steps_loop_iteration_reduce(ctx: &str, body: &serde_json::Value) {
    let ctl_label = body_str(body, "ctl", ctx);
    let expect = body_str(body, "expect", ctx);
    let ctl = match ctl_label {
        "advance" => StagedStepIterationCtl::AdvanceToNextStep {
            n: body["n"].as_u64().unwrap_or(1) as usize,
            completed_steps: body["completed"].as_u64().unwrap_or(1) as usize,
        },
        "retry" => StagedStepIterationCtl::RetryCurrentStep {
            n: body["n"].as_u64().unwrap_or(1) as usize,
        },
        "cancelled" => StagedStepIterationCtl::CancelledAfterOuterOk,
        other => panic!("{ctx}: unknown ctl {other}"),
    };
    let action = reduce_steps_loop_iteration_ctl(ctl);
    assert_eq!(
        match action {
            super::steps_loop_reduce::StepsLoopIterationReduceAction::RetryCurrentStep {
                ..
            } => {
                "retry_current_step"
            }
            super::steps_loop_reduce::StepsLoopIterationReduceAction::AdvanceToNextStep {
                ..
            } => {
                "advance_to_next_step"
            }
            super::steps_loop_reduce::StepsLoopIterationReduceAction::BreakCancelled => {
                "break_cancelled"
            }
        },
        expect,
        "{ctx}: steps loop iteration"
    );
}

fn assert_prepared_stagnation_reduce(ctx: &str, body: &serde_json::Value) {
    let entered = body["entered"].as_bool().unwrap_or(false);
    let step_count = body["step_count"].as_u64().unwrap_or(1) as usize;
    let expect = body_str(body, "expect", ctx);
    let plan = AgentReplyPlanV1 {
        plan_type: "agent_reply_plan".into(),
        version: 1,
        steps: (0..step_count)
            .map(|i| crate::agent::plan_artifact::PlanStepV1 {
                id: format!("s{i}"),
                description: "d".into(),
                workflow_node_id: None,
                executor_kind: None,
                step_kind: None,
                acceptance: None,
                max_step_retries: None,
                transitions: None,
            })
            .collect(),
        no_task: false,
    };
    let action = reduce_prepared_stagnation_after_parse(&[], &plan, entered);
    assert_eq!(action.as_str(), expect, "{ctx}: prepared stagnation");
}

fn assert_completed_replanning_reduce(ctx: &str, body: &serde_json::Value) {
    let entered = body["entered"].as_bool().unwrap_or(false);
    let step_count = body["step_count"].as_u64().unwrap_or(1) as usize;
    let expect = body_str(body, "expect", ctx);
    let steps: Vec<_> = (0..step_count)
        .map(|i| crate::agent::plan_artifact::PlanStepV1 {
            id: format!("s{i}"),
            description: "verify".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: Some("verify".into()),
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        })
        .collect();
    let action = reduce_completed_replanning_suppression(&[], entered, &steps);
    assert_eq!(
        match action {
            super::completed_replanning_reduce::CompletedReplanningReduceAction::ContinuePostParse => {
                "continue_post_parse"
            }
            super::completed_replanning_reduce::CompletedReplanningReduceAction::FinishQuiet => {
                "finish_quiet"
            }
        },
        expect,
        "{ctx}: completed replanning"
    );
}

fn assert_steps_loop_post_outer(ctx: &str, body: &serde_json::Value) {
    let run_ok = body["run_ok"].as_bool().unwrap_or(true);
    let verify_fail = body["verify_fail"].as_bool().unwrap_or(false);
    let cancelled = body["cancelled"].as_bool().unwrap_or(false);
    let tools_ok = body["tools_ok"].as_bool().unwrap_or(true);
    let patch_planner = body["patch_planner"].as_bool().unwrap_or(false);
    let expect = body_str(body, "expect", ctx);
    let run_step = if run_ok {
        Ok(())
    } else {
        Err(RunAgentTurnError::Other {
            phase: AgentTurnSubPhase::Executor,
            message: "golden".into(),
        })
    };
    let verify_reason = verify_fail.then(|| "golden_verify".to_string());
    let got = resolve_staged_step_post_outer_route_from_results(
        &run_step,
        &verify_reason,
        cancelled,
        tools_ok,
        patch_planner,
    );
    assert_eq!(got.as_str(), expect, "{ctx}: post outer route");
}

fn post_outer_route_from_label(label: &str) -> StagedStepPostOuterRoute {
    match label {
        "exec_or_verify_failed" => StagedStepPostOuterRoute::ExecOrVerifyFailed,
        "cancelled" => StagedStepPostOuterRoute::Cancelled,
        "tool_failure_patch" => StagedStepPostOuterRoute::ToolFailurePatch,
        "emit_success" => StagedStepPostOuterRoute::EmitSuccess,
        other => panic!("unknown post outer route label: {other}"),
    }
}

fn assert_step_iteration_reduce(ctx: &str, body: &serde_json::Value) {
    let route_label = body_str(body, "route", ctx);
    let expect = body_str(body, "expect", ctx);
    let route = post_outer_route_from_label(route_label);
    assert_eq!(
        reduce_staged_step_post_outer_route(route).as_str(),
        expect,
        "{ctx}: step iteration reduce"
    );
}

fn post_parse_schedule_from_label(label: &str) -> PreparedPostParseSchedule {
    match label {
        "no_task_then_outer" => PreparedPostParseSchedule::NoTaskThenOuter,
        "full_pipeline_then_steps" => PreparedPostParseSchedule::FullPipelineThenSteps,
        other => panic!("unknown post parse schedule label: {other}"),
    }
}

fn assert_orchestrator_post_parse_schedule(ctx: &str, body: &serde_json::Value) {
    let schedule = post_parse_schedule_from_label(body_str(body, "schedule", ctx));
    let expect = body_str(body, "expect_phase", ctx);
    assert_eq!(
        orchestrator_phase_for_post_parse_schedule(schedule).as_str(),
        expect,
        "{ctx}: post parse schedule"
    );
}

fn ensemble_route_from_label(label: &str) -> StagedPlanEnsembleRoute {
    match label {
        "skip_not_configured" => StagedPlanEnsembleRoute::SkipNotConfigured,
        "skip_validate_only_binding" => StagedPlanEnsembleRoute::SkipValidateOnlyBinding,
        "skip_casual_heuristic" => StagedPlanEnsembleRoute::SkipCasualHeuristic,
        "run" => StagedPlanEnsembleRoute::Run,
        other => panic!("unknown ensemble route label: {other}"),
    }
}

fn optimizer_route_from_label(label: &str) -> StagedPlanOptimizerRoute {
    match label {
        "skip_steps_lt2" => StagedPlanOptimizerRoute::SkipStepsLt2,
        "skip_optimizer_round_disabled" => StagedPlanOptimizerRoute::SkipOptimizerRoundDisabled,
        "skip_validate_only_binding" => StagedPlanOptimizerRoute::SkipValidateOnlyBinding,
        "skip_no_parallel_tools" => StagedPlanOptimizerRoute::SkipNoParallelTools,
        "run" => StagedPlanOptimizerRoute::Run,
        other => panic!("unknown optimizer route label: {other}"),
    }
}

fn full_pipeline_segment_from_label(label: &str) -> FullPipelineSegment {
    match label {
        "ensemble" => FullPipelineSegment::Ensemble,
        "optimizer" => FullPipelineSegment::Optimizer,
        "nl_followup" => FullPipelineSegment::NlFollowup,
        other => panic!("unknown full pipeline segment label: {other}"),
    }
}

fn assert_full_pipeline_segment_reduce(ctx: &str, body: &serde_json::Value) {
    let segment = full_pipeline_segment_from_label(body_str(body, "segment", ctx));
    let schedule = PreparedFullPipelineSchedule {
        ensemble_route: ensemble_route_from_label(body_str(body, "ensemble_route", ctx)),
        optimizer_route: optimizer_route_from_label(body_str(body, "optimizer_route", ctx)),
        nl_followup_before_steps: body["nl_followup"].as_bool().unwrap_or(false),
    };
    let expect = body_str(body, "expect", ctx);
    assert_eq!(
        reduce_full_pipeline_segment(segment, &schedule).as_str(),
        expect,
        "{ctx}: full pipeline segment reduce"
    );
}

fn assert_step_patch_recover_reduce(ctx: &str, body: &serde_json::Value) {
    let branch = match body_str(body, "branch", ctx) {
        "outer_exec_or_verify" => StepPatchRecoverBranch::OuterExecOrVerify,
        "tool_failure" => StepPatchRecoverBranch::ToolFailure,
        other => panic!("{ctx}: unknown patch recover branch {other}"),
    };
    let feedback_mode = match body_str(body, "feedback_mode", ctx) {
        "fail_fast" => StagedPlanFeedbackMode::FailFast,
        "patch_planner" => StagedPlanFeedbackMode::PatchPlanner,
        other => panic!("{ctx}: unknown feedback_mode {other}"),
    };
    let patch_max = body["patch_max"].as_u64().unwrap_or(0) as usize;
    let verify_reason = body.get("verify_reason").and_then(|v| {
        if v.is_null() {
            None
        } else {
            Some(v.as_str().expect("verify_reason str").to_string())
        }
    });
    let has_outer_error = body["has_outer_error"].as_bool().unwrap_or(false);
    let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
        branch,
        feedback_mode,
        step_max_retries: None,
        staged_plan_patch_max_attempts: patch_max,
        step_verify_failed_reason: verify_reason,
        has_outer_loop_error: has_outer_error,
    });
    let expect = body_str(body, "expect", ctx);
    match expect {
        "skip" => assert_eq!(action, StepPatchRecoverReduceAction::Skip, "{ctx}"),
        "run_patch" => assert!(
            matches!(action, StepPatchRecoverReduceAction::Run(_)),
            "{ctx}: expected run_patch got {action:?}"
        ),
        other => panic!("{ctx}: unknown expect {other}"),
    }
}

fn assert_prepared_route_reduce(ctx: &str, body: &serde_json::Value) {
    let route = prepared_route_from_label(body_str(body, "route", ctx));
    let expect = body_str(body, "expect", ctx);
    assert_eq!(
        reduce_prepared_planner_route(&route).as_str(),
        expect,
        "{ctx}: prepared route reduce"
    );
}

fn assert_orchestrator_prepared_route_reduce(ctx: &str, body: &serde_json::Value) {
    let route = prepared_route_from_label(body_str(body, "route", ctx));
    let action = reduce_prepared_planner_route(&route);
    let expect = body_str(body, "expect_phase", ctx);
    assert_eq!(
        orchestrator_phase_for_prepared_route_reduce(action).as_str(),
        expect,
        "{ctx}: prepared route reduce → orchestrator phase"
    );
}

fn assert_orchestrator_step_iteration_reduce(ctx: &str, body: &serde_json::Value) {
    let route_label = body_str(body, "route", ctx);
    let route = match route_label {
        "exec_or_verify_failed" => StagedStepPostOuterRoute::ExecOrVerifyFailed,
        "cancelled" => StagedStepPostOuterRoute::Cancelled,
        "tool_failure_patch" => StagedStepPostOuterRoute::ToolFailurePatch,
        "emit_success" => StagedStepPostOuterRoute::EmitSuccess,
        other => panic!("{ctx}: unknown step iteration reduce route {other}"),
    };
    let action = reduce_staged_step_post_outer_route(route);
    let expect = body_str(body, "expect_phase", ctx);
    assert_eq!(
        orchestrator_phase_for_step_iteration_reduce(action).as_str(),
        expect,
        "{ctx}: step iteration reduce → orchestrator phase"
    );
}

fn rolling_horizon_preflight_from_body(
    body: &serde_json::Value,
    ctx: &str,
) -> RollingHorizonPreflightInput {
    RollingHorizonPreflightInput {
        staged_rounds: body["staged_rounds"].as_u64().expect("staged_rounds") as usize,
        max_rounds: body["max_rounds"].as_u64().expect("max_rounds") as usize,
        phase: turn_phase_from_label(body_str(body, "phase", ctx)),
        early_stop_allow: body["early_stop_allow"].as_bool().unwrap_or(false),
    }
}

fn assert_rolling_horizon_preflight_reduce(ctx: &str, body: &serde_json::Value) {
    let input = rolling_horizon_preflight_from_body(body, ctx);
    let expect = body_str(body, "expect", ctx);
    assert_eq!(
        reduce_rolling_horizon_preflight(input).as_str(),
        expect,
        "{ctx}: rolling horizon preflight reduce"
    );
}

fn assert_orchestrator_rolling_horizon_preflight(ctx: &str, body: &serde_json::Value) {
    let input = rolling_horizon_preflight_from_body(body, ctx);
    let action = reduce_rolling_horizon_preflight(input);
    let expect = body_str(body, "expect_phase", ctx);
    assert_eq!(
        orchestrator_phase_for_rolling_horizon_preflight(action).as_str(),
        expect,
        "{ctx}: rolling horizon preflight → orchestrator phase"
    );
}

fn assert_step_patch_failure_kind(ctx: &str, body: &serde_json::Value) {
    let expect = body_str(body, "expect", ctx);
    if let Some(kind_label) = body.get("kind").and_then(|v| v.as_str()) {
        assert_eq!(
            kind_label,
            StagedStepPatchFailureKind::ToolMessagesNotOk.as_str(),
            "{ctx}"
        );
        return;
    }
    let verify_reason = body
        .get("verify_reason")
        .map(|v| {
            if v.is_null() {
                None
            } else {
                Some(v.as_str().expect("verify_reason str").to_string())
            }
        })
        .unwrap_or(None);
    let has_outer_error = body["has_outer_error"].as_bool().unwrap_or(false);
    let got = resolve_staged_step_patch_failure_kind(&verify_reason, has_outer_error)
        .map(|k| k.as_str().to_string())
        .unwrap_or_else(|| "none".to_string());
    assert_eq!(got, expect, "{ctx}: patch failure kind");
}

fn assert_golden_fsm_line(ctx: &str, row: &GoldenLine) {
    match row.case.as_str() {
        "orchestrator_prepared_route" => assert_orchestrator_prepared_route(ctx, &row.body),
        "orchestrator_turn_phase" => assert_orchestrator_turn_phase(ctx, &row.body),
        "orchestrator_full_pipeline" => assert_orchestrator_full_pipeline(ctx, &row.body),
        "orchestrator_steps_loop_trace" => {
            assert_orchestrator_steps_loop_trace(ctx, &row.body);
        }
        "outer_loop_reflect_ctl" => assert_outer_loop_reflect_ctl(ctx, &row.body),
        "outer_loop_reflect_reduce" => assert_outer_loop_reflect_reduce(ctx, &row.body),
        "outer_loop_post_tools_reduce" => assert_outer_loop_post_tools_reduce(ctx, &row.body),
        "outer_loop_iteration_exit" => assert_outer_loop_iteration_exit(ctx, &row.body),
        "steps_loop_post_outer" => assert_steps_loop_post_outer(ctx, &row.body),
        "steps_loop_preflight_reduce" => assert_steps_loop_preflight_reduce(ctx, &row.body),
        "steps_loop_iteration_reduce" => assert_steps_loop_iteration_reduce(ctx, &row.body),
        "prepared_stagnation_reduce" => assert_prepared_stagnation_reduce(ctx, &row.body),
        "completed_replanning_reduce" => assert_completed_replanning_reduce(ctx, &row.body),
        "step_iteration_reduce" => assert_step_iteration_reduce(ctx, &row.body),
        "prepared_route_reduce" => assert_prepared_route_reduce(ctx, &row.body),
        "orchestrator_post_parse_schedule" => {
            assert_orchestrator_post_parse_schedule(ctx, &row.body);
        }
        "step_patch_failure_kind" => assert_step_patch_failure_kind(ctx, &row.body),
        "orchestrator_prepared_route_reduce" => {
            assert_orchestrator_prepared_route_reduce(ctx, &row.body);
        }
        "orchestrator_step_iteration_reduce" => {
            assert_orchestrator_step_iteration_reduce(ctx, &row.body);
        }
        "rolling_horizon_preflight_reduce" => {
            assert_rolling_horizon_preflight_reduce(ctx, &row.body);
        }
        "rolling_horizon_advance_reduce" => {
            assert_rolling_horizon_advance_reduce(ctx, &row.body);
        }
        "orchestrator_rolling_horizon_preflight" => {
            assert_orchestrator_rolling_horizon_preflight(ctx, &row.body);
        }
        "full_pipeline_segment_reduce" => assert_full_pipeline_segment_reduce(ctx, &row.body),
        "step_patch_recover_reduce" => assert_step_patch_recover_reduce(ctx, &row.body),
        other => panic!("{ctx}: unknown case {other}"),
    }
}

#[test]
fn golden_fsm_orchestrator_lines_match_mappings() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/fsm_orchestrator_golden.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ctx = line_ctx(path.as_path(), line_no, &row.id);
        assert_golden_fsm_line(&ctx, &row);
    }
}
