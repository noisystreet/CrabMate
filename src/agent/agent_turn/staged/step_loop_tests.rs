use std::collections::HashMap;

use super::*;
use crate::agent::agent_turn::errors::{AgentTurnSubPhase, RunAgentTurnError};
use crate::agent::plan_artifact::{PlanStepExecutorKind, PlanStepV1};
use crate::types::{Message, MessageContent};

// --- staged_step_fsm ---

#[test]
fn budget_matches_loop_upper_bound() {
    assert_eq!(staged_patch_budget_after_step_failure(Some(5), 10), 5);
    assert_eq!(staged_patch_budget_after_step_failure(None, 7), 7);
    assert_eq!(staged_patch_budget_after_step_failure(Some(0), 3), 0);
    assert_eq!(staged_patch_budget_tool_messages_not_ok(0), 0);
}

#[test]
fn patch_only_in_patch_planner_mode() {
    assert!(staged_step_patch_planner_enabled(
        StagedPlanFeedbackMode::PatchPlanner
    ));
    assert!(!staged_step_patch_planner_enabled(
        StagedPlanFeedbackMode::FailFast
    ));
}

// --- step_loop_fsm ---

fn step_with_transition(
    id: &str,
    condition: &str,
    target: &str,
    max_loops: Option<u32>,
) -> PlanStepV1 {
    PlanStepV1 {
        id: id.to_string(),
        description: "d".to_string(),
        workflow_node_id: None,
        executor_kind: None,
        step_kind: None,
        acceptance: None,
        max_step_retries: None,
        transitions: Some(vec![PlanStepControlFlow {
            condition: condition.to_string(),
            target_step_id: target.to_string(),
            max_loops,
        }]),
    }
}

#[test]
fn transition_respects_max_loops() {
    let step = step_with_transition("a", "on_verify_success", "b", Some(1));
    let mut counters = HashMap::new();
    assert!(staged_step_transition_trigger(&step, false, &None, &mut counters).is_some());
    assert!(staged_step_transition_trigger(&step, false, &None, &mut counters).is_none());
}

#[test]
fn jump_truncates_and_suffixes_ids() {
    let original = vec![
        PlanStepV1 {
            id: "s0".into(),
            description: "".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        },
        PlanStepV1 {
            id: "s1".into(),
            description: "".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        },
    ];
    let mut plan_steps = original.clone();
    let step = step_with_transition("cur", "always", "s1", None);
    let mut counters = HashMap::new();
    let r = try_apply_staged_plan_control_flow_jump(
        &step,
        0,
        &mut plan_steps,
        original.as_slice(),
        &mut counters,
        false,
        &None,
    );
    assert!(r.is_some());
    assert_eq!(plan_steps.len(), 2);
    assert_eq!(plan_steps[0].id, "s0");
    assert_eq!(plan_steps[1].id, "s1-loop0");
}

#[test]
fn unknown_target_returns_none_without_truncating() {
    let original = vec![PlanStepV1 {
        id: "only".into(),
        description: "".into(),
        workflow_node_id: None,
        executor_kind: None,
        step_kind: None,
        acceptance: None,
        max_step_retries: None,
        transitions: None,
    }];
    let mut plan_steps = vec![
        PlanStepV1 {
            id: "a".into(),
            description: "".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        },
        PlanStepV1 {
            id: "b".into(),
            description: "".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        },
    ];
    let step = step_with_transition("x", "always", "missing", None);
    let mut counters = HashMap::new();
    let before_len = plan_steps.len();
    let r = try_apply_staged_plan_control_flow_jump(
        &step,
        0,
        &mut plan_steps,
        original.as_slice(),
        &mut counters,
        false,
        &None,
    );
    assert!(r.is_none());
    assert_eq!(plan_steps.len(), before_len);
}

#[test]
fn injected_body_contains_step_meta() {
    let step = PlanStepV1 {
        id: "sid".into(),
        description: "desc".into(),
        workflow_node_id: None,
        executor_kind: Some(PlanStepExecutorKind::ReviewReadonly),
        step_kind: None,
        acceptance: None,
        max_step_retries: None,
        transitions: None,
    };
    let body = staged_injected_step_user_body(1, 3, &step, None);
    assert!(body.contains("### 分步 1/3"));
    assert!(body.contains("sid"));
    assert!(body.contains("desc"));
    assert!(body.contains("review_readonly"));
}

#[test]
fn injected_body_prefixes_immutable_goal_when_present() {
    let step = PlanStepV1 {
        id: "sid".into(),
        description: "desc".into(),
        workflow_node_id: None,
        executor_kind: None,
        step_kind: None,
        acceptance: None,
        max_step_retries: None,
        transitions: None,
    };
    let body = staged_injected_step_user_body(1, 2, &step, Some("用户总问句"));
    assert!(body.contains("【不变层·本轮用户总目标】"));
    assert!(body.contains("用户总问句"));
    assert!(body.contains("### 分步 1/2"));
}

// --- step_iteration_fsm ---

#[test]
fn after_outer_loop_err_skips_verify() {
    let err = Err(RunAgentTurnError::Other {
        phase: AgentTurnSubPhase::Executor,
        message: "x".into(),
    });
    let r = staged_step_after_outer_loop(&err, &Some("verify".into()));
    assert_eq!(
        r,
        StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: Some("x".into()),
            verify_failure_reason: None,
        }
    );
}

#[test]
fn after_outer_loop_ok_and_verify_fail() {
    let ok = Ok(());
    let r = staged_step_after_outer_loop(&ok, &Some("bad".into()));
    assert_eq!(
        r,
        StagedStepAfterOuterLoop::ExecutionOrVerifyFailed {
            outer_loop_error: None,
            verify_failure_reason: Some("bad".into()),
        }
    );
}

#[test]
fn after_outer_loop_proceed() {
    let ok = Ok(());
    assert_eq!(
        staged_step_after_outer_loop(&ok, &None),
        StagedStepAfterOuterLoop::ProceedToToolCheck
    );
}

#[test]
fn exhausted_message_prefers_outer_err() {
    let err = Err(RunAgentTurnError::Other {
        phase: AgentTurnSubPhase::Executor,
        message: "oe".into(),
    });
    assert_eq!(
        staged_step_failure_retry_exhausted_message(&err, &Some("v".into())),
        "oe"
    );
}

#[test]
fn exhausted_message_verify_or_default() {
    let ok = Ok(());
    assert_eq!(
        staged_step_failure_retry_exhausted_message(&ok, &Some("vf".into())),
        "vf"
    );
    assert_eq!(
        staged_step_failure_retry_exhausted_message(&ok, &None),
        "局部修复耗尽上限"
    );
}

#[test]
fn verify_fail_patch_detail_includes_acceptance_reference() {
    use crate::agent::plan_artifact::PlanStepAcceptance;
    let acc = PlanStepAcceptance {
        expect_exit_code: Some(0),
        expect_stdout_contains: Some("needle".into()),
        expect_stderr_contains: None,
        expect_file_exists: None,
        expect_json_path_equals: None,
        expect_http_status: None,
    };
    let d =
        staged_step_verify_fail_patch_detail("exit_code_mismatch: expected 0, got 1", Some(&acc));
    assert!(d.contains("expect_exit_code=0"));
    assert!(d.contains("expect_stdout_contains=needle"));
    assert!(d.contains("exit_code_mismatch"));
}

#[test]
fn verify_fail_patch_detail_without_acceptance_reference() {
    let d = staged_step_verify_fail_patch_detail("no tool result", None);
    assert!(d.contains("no tool result"));
    assert!(!d.contains("参考验收"));
}

#[test]
fn tool_fail_patch_detail_summarizes_failed_tools() {
    let tool_fail = Message {
        role: "tool".to_string(),
        content: Some(MessageContent::Text("退出码：1\n标准错误：\nfail\n".into())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some("read_file".to_string()),
        tool_call_id: None,
    };
    let messages = vec![
        Message::user_only("step"),
        tool_fail,
        Message::user_only("next"),
    ];
    let d = staged_step_tool_failure_patch_detail(&messages, 0, None);
    assert!(d.contains("read_file"));
    assert!(d.contains("偏差结构化"));
}

#[test]
fn tool_fail_patch_detail_includes_acceptance_reference() {
    use crate::agent::plan_artifact::PlanStepAcceptance;
    let acc = PlanStepAcceptance {
        expect_exit_code: Some(0),
        expect_stdout_contains: Some("ok".into()),
        expect_stderr_contains: None,
        expect_file_exists: None,
        expect_json_path_equals: None,
        expect_http_status: None,
    };
    let tool_fail = Message {
        role: "tool".to_string(),
        content: Some(MessageContent::Text("退出码：1\n".into())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some("run_command".to_string()),
        tool_call_id: None,
    };
    let messages = vec![Message::user_only("step"), tool_fail];
    let d = staged_step_tool_failure_patch_detail(&messages, 0, Some(&acc));
    assert!(d.contains("expect_exit_code=0"));
    assert!(d.contains("run_command"));
}

#[test]
fn tool_fail_patch_detail_forbids_repeat_short_circuit_retry() {
    let raw =
        "错误：检测到同命令重复失败，已短路本次调用（error=run_command_failed）。请切换策略。";
    let parsed = crate::tool_result::parse_legacy_output("run_command", raw);
    let envelope = crate::tool_result::encode_tool_message_envelope_v1(
        "run_command",
        "make".into(),
        &parsed,
        raw,
        None,
    );
    let tool_fail = Message {
        role: "tool".to_string(),
        content: Some(MessageContent::Text(envelope)),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: Some("run_command".to_string()),
        tool_call_id: None,
    };
    let messages = vec![Message::user_only("step"), tool_fail];
    let d = staged_step_tool_failure_patch_detail(&messages, 0, None);

    assert!(d.contains("硬约束"));
    assert!(d.contains("error_code=repeated_tool_failure_short_circuit"));
    assert!(d.contains("不得再次生成相同 `run_command`"));
}

#[test]
fn exec_fail_patch_detail_includes_error_tail() {
    let d = staged_step_exec_fail_patch_detail("context canceled");
    assert!(d.contains(STAGED_STEP_OUTER_LOOP_FAIL_DETAIL));
    assert!(d.contains("context canceled"));
}

#[test]
fn tool_phase_routes() {
    assert_eq!(
        staged_step_tool_phase_route(true, false),
        StagedStepToolPhaseRoute::EmitStepSuccess
    );
    assert_eq!(
        staged_step_tool_phase_route(true, true),
        StagedStepToolPhaseRoute::EmitStepSuccess
    );
    assert_eq!(
        staged_step_tool_phase_route(false, false),
        StagedStepToolPhaseRoute::EmitStepSuccess
    );
    assert_eq!(
        staged_step_tool_phase_route(false, true),
        StagedStepToolPhaseRoute::AttemptToolFailurePatches
    );
}

#[test]
fn wall_clock_exceeded_matches_loop() {
    assert!(!staged_step_wall_clock_exceeded(0, 999));
    assert!(!staged_step_wall_clock_exceeded(10, 10));
    assert!(staged_step_wall_clock_exceeded(10, 11));
}

// --- steps_loop_route_fsm ---

#[test]
fn route_table_matches_legacy_branches() {
    let ok = Ok(());
    let err = Err(RunAgentTurnError::Other {
        phase: AgentTurnSubPhase::Executor,
        message: "x".into(),
    });
    assert_eq!(
        resolve_staged_step_post_outer_route_from_results(&err, &None, false, true, false),
        StagedStepPostOuterRoute::ExecOrVerifyFailed
    );
    assert_eq!(
        resolve_staged_step_post_outer_route_from_results(
            &ok,
            &Some("vf".into()),
            false,
            true,
            false
        ),
        StagedStepPostOuterRoute::ExecOrVerifyFailed
    );
    assert_eq!(
        resolve_staged_step_post_outer_route_from_results(&ok, &None, true, true, false),
        StagedStepPostOuterRoute::Cancelled
    );
    assert_eq!(
        resolve_staged_step_post_outer_route_from_results(&ok, &None, false, false, true),
        StagedStepPostOuterRoute::ToolFailurePatch
    );
    assert_eq!(
        resolve_staged_step_post_outer_route_from_results(&ok, &None, false, false, false),
        StagedStepPostOuterRoute::EmitSuccess
    );
    assert_eq!(
        resolve_staged_step_post_outer_route_from_results(&ok, &None, false, true, false),
        StagedStepPostOuterRoute::EmitSuccess
    );
}

// --- step_iteration_reduce ---

#[test]
fn reduce_matches_post_outer_route_table() {
    let ok = Ok(());
    let cases = [
        (
            resolve_staged_step_post_outer_route_from_results(
                &Err(RunAgentTurnError::Other {
                    phase: AgentTurnSubPhase::Executor,
                    message: "x".into(),
                }),
                &None,
                false,
                true,
                false,
            ),
            StepIterationReduceAction::ExecOrVerifyFailed,
        ),
        (
            resolve_staged_step_post_outer_route_from_results(&ok, &None, true, true, false),
            StepIterationReduceAction::Cancelled,
        ),
        (
            resolve_staged_step_post_outer_route_from_results(&ok, &None, false, false, true),
            StepIterationReduceAction::ToolFailurePatch,
        ),
        (
            resolve_staged_step_post_outer_route_from_results(&ok, &None, false, true, false),
            StepIterationReduceAction::EmitSuccessAdvance,
        ),
    ];
    for (route, expect) in cases {
        assert_eq!(reduce_staged_step_post_outer_route(route), expect);
        assert_eq!(route.reduce_action(), expect);
    }
}

// --- steps_loop_reduce ---

#[test]
fn preflight_breaks_on_cancel() {
    assert_eq!(
        reduce_steps_loop_preflight(false, true),
        StepsLoopPreflightReduceAction::BreakCancelled
    );
}

#[test]
fn iteration_ctl_maps_advance() {
    assert_eq!(
        reduce_steps_loop_iteration_ctl(StagedStepIterationCtl::AdvanceToNextStep {
            n: 2,
            completed_steps: 1,
        }),
        StepsLoopIterationReduceAction::AdvanceToNextStep {
            n: 2,
            completed_steps: 1,
        }
    );
}

#[test]
fn separator_when_empty_or_cancelled_before_progress() {
    assert!(should_push_steps_loop_separator(0, false, 0));
    assert!(should_push_steps_loop_separator(3, true, 0));
    assert!(!should_push_steps_loop_separator(3, true, 2));
}

// --- step_patch_route_fsm ---

#[test]
fn resolve_verify_fail_kind() {
    let kind = resolve_staged_step_patch_failure_kind(&Some("exit_code_mismatch".into()), false)
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

// --- step_patch_recover_reduce ---

#[test]
fn outer_skip_when_fail_fast() {
    let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
        branch: StepPatchRecoverBranch::OuterExecOrVerify,
        feedback_mode: StagedPlanFeedbackMode::FailFast,
        step_max_retries: None,
        staged_plan_patch_max_attempts: 2,
        step_verify_failed_reason: Some("exit_code_mismatch".into()),
        has_outer_loop_error: false,
    });
    assert_eq!(action, StepPatchRecoverReduceAction::Skip);
}

#[test]
fn outer_run_on_verify_fail() {
    let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
        branch: StepPatchRecoverBranch::OuterExecOrVerify,
        feedback_mode: StagedPlanFeedbackMode::PatchPlanner,
        step_max_retries: None,
        staged_plan_patch_max_attempts: 2,
        step_verify_failed_reason: Some("exit_code_mismatch".into()),
        has_outer_loop_error: false,
    });
    assert_eq!(
        action,
        StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
            failure_kind: StagedStepPatchFailureKind::StepVerifyFail {
                reason: "exit_code_mismatch".into(),
                empty_execution: false,
            },
            patch_budget: 2,
            steps_loop_phase: "patch_replanner_attempt",
        })
    );
}

#[test]
fn tool_branch_always_runs_with_budget() {
    let action = reduce_step_patch_recover(StepPatchRecoverReduceInput {
        branch: StepPatchRecoverBranch::ToolFailure,
        feedback_mode: StagedPlanFeedbackMode::PatchPlanner,
        step_max_retries: None,
        staged_plan_patch_max_attempts: 3,
        step_verify_failed_reason: None,
        has_outer_loop_error: false,
    });
    assert_eq!(
        action,
        StepPatchRecoverReduceAction::Run(StepPatchRecoverPlan {
            failure_kind: StagedStepPatchFailureKind::ToolMessagesNotOk,
            patch_budget: 3,
            steps_loop_phase: "patch_replanner_tool_failure",
        })
    );
}
