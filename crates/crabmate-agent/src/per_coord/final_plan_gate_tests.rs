use super::*;
use crate::plan_rewrite::PlanRewriteExhaustedReason;
use crate::plan_semantic::PlanSemanticLlmOutcome;
use crabmate_types::{FunctionCall, MessageContent, ToolCall};

fn minimal_cfg() -> AgentConfig {
    crabmate_config::load_config(None).expect("embed default config")
}

fn gate_args<'a>(
    msg: &'a Message,
    messages: &'a [Message],
    cfg: &'a AgentConfig,
    policy: FinalPlanRequirementMode,
    source: PlanRequirementSource,
    attempts: usize,
    max_attempts: usize,
) -> FinalPlanGateArgs<'a> {
    FinalPlanGateArgs {
        msg,
        messages,
        cfg,
        workspace_is_set: false,
        gate_context: build_final_plan_gate_context(policy, source),
        final_plan_require_strict_workflow_node_coverage: false,
        final_plan_semantic_check_enabled: false,
        final_plan_semantic_check_max_non_readonly_tools: 0,
        layer_need: None,
        validate_only_binding_ids: None,
        plan_rewrite_attempts: attempts,
        plan_rewrite_max_attempts: max_attempts,
    }
}

#[test]
fn gate_route_accept_ok_when_plan_valid() {
    let cfg = minimal_cfg();
    let ok = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"x"}]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let hist: Vec<Message> = vec![];
    let o = step_check_structured_plan(gate_args(
        &ok,
        &hist,
        &cfg,
        FinalPlanRequirementMode::WorkflowReflection,
        PlanRequirementSource::WorkflowReflection,
        0,
        2,
    ));
    assert_eq!(o.route, FinalPlanGateRoute::AcceptStructuredPlanOk);
    assert_eq!(
        o.decision_reason,
        FinalPlanGateDecisionReason::StructuredPlanAccepted
    );
    assert!(matches!(o.after, AfterFinalAssistant::StopTurn));
}

#[test]
fn gate_route_rewrite_when_parse_fails() {
    let cfg = minimal_cfg();
    let bad = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no json plan".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let hist: Vec<Message> = vec![];
    let o = step_check_structured_plan(gate_args(
        &bad,
        &hist,
        &cfg,
        FinalPlanRequirementMode::WorkflowReflection,
        PlanRequirementSource::WorkflowReflection,
        0,
        2,
    ));
    assert_eq!(o.route, FinalPlanGateRoute::SemanticsFailedRequestRewrite);
    assert_eq!(
        o.decision_reason,
        FinalPlanGateDecisionReason::PlanParseFailed
    );
    match &o.after {
        AfterFinalAssistant::RequestPlanRewrite(m) => {
            let body = crabmate_types::message_content_as_str(&m.content).unwrap_or("");
            assert!(
                body.contains("校验反馈") && body.contains("not_found"),
                "parse-fail rewrite should echo error code; got:\n{body}"
            );
            assert!(
                !body.contains("expect_json_path_equals"),
                "rewrite must stay brief"
            );
        }
        other => panic!("expected RequestPlanRewrite, got {other:?}"),
    }
    assert_eq!(o.next_plan_rewrite_count, Some(1));
}

#[test]
fn gate_route_rewrite_when_layer_count_mismatches_includes_feedback() {
    let cfg = minimal_cfg();
    let one_step = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"only one step"}]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let hist: Vec<Message> = vec![];
    let mut args = gate_args(
        &one_step,
        &hist,
        &cfg,
        FinalPlanRequirementMode::WorkflowReflection,
        PlanRequirementSource::WorkflowReflection,
        0,
        2,
    );
    args.layer_need = Some(2);
    let o = step_check_structured_plan(args);
    assert_eq!(o.route, FinalPlanGateRoute::SemanticsFailedRequestRewrite);
    assert_eq!(
        o.decision_reason,
        FinalPlanGateDecisionReason::StaticSemanticsFailed
    );
    match &o.after {
        AfterFinalAssistant::RequestPlanRewrite(m) => {
            let body = crabmate_types::message_content_as_str(&m.content).unwrap_or("");
            assert!(
                body.contains("校验反馈")
                    && body.contains("plan_layer_count_mismatch")
                    && body.contains("need=2")
                    && body.contains("got=1"),
                "layer-mismatch rewrite should echo feedback codes; got:\n{body}"
            );
            assert!(
                !body.contains("expect_json_path_equals"),
                "rewrite must stay brief"
            );
        }
        other => panic!("expected RequestPlanRewrite, got {other:?}"),
    }
    assert_eq!(o.next_plan_rewrite_count, Some(1));
}

#[test]
fn gate_route_exhausted_when_attempts_maxed() {
    let cfg = minimal_cfg();
    let bad = Message {
        role: "assistant".to_string(),
        content: Some(MessageContent::Text("no json plan".to_string())),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let hist: Vec<Message> = vec![];
    let o = step_check_structured_plan(gate_args(
        &bad,
        &hist,
        &cfg,
        FinalPlanRequirementMode::WorkflowReflection,
        PlanRequirementSource::WorkflowReflection,
        2,
        2,
    ));
    assert_eq!(o.route, FinalPlanGateRoute::SemanticsFailedRewriteExhausted);
    assert!(matches!(
        o.after,
        AfterFinalAssistant::StopTurnPlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanMissing
        }
    ));
}

#[test]
fn resolve_phase_never_is_no_requirement() {
    let ctx = build_final_plan_gate_context(
        FinalPlanRequirementMode::Never,
        PlanRequirementSource::WorkflowReflection,
    );
    assert_eq!(ctx.phase, FinalPlanGatePhase::NoRequirement);
}

#[test]
fn run_gate_no_requirement_returns_stop_turn() {
    let cfg = minimal_cfg();
    let msg = Message {
        role: "assistant".to_string(),
        content: Some("x".into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let o = run_final_plan_gate(
        FinalPlanGatePhase::NoRequirement,
        FinalPlanGateEvent::FinalAssistantArrived,
        gate_args(
            &msg,
            &[],
            &cfg,
            FinalPlanRequirementMode::Never,
            PlanRequirementSource::None,
            0,
            2,
        ),
    );
    assert_eq!(o.route, FinalPlanGateRoute::StopNoRequirement);
    assert_eq!(
        o.decision_reason,
        FinalPlanGateDecisionReason::PolicyNoRequirement
    );
    assert!(matches!(o.after, AfterFinalAssistant::StopTurn));
}

#[test]
fn gate_route_pending_semantic_when_digest_present() {
    let cfg = minimal_cfg();
    let ok = Message {
        role: "assistant".to_string(),
        content: Some(
            r#"```json
{"type":"agent_reply_plan","version":1,"steps":[{"id":"s1","description":"x"}]}
```"#
                .into(),
        ),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    };
    let hist = vec![
        Message {
            role: "assistant".to_string(),
            content: None,
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: Some(vec![ToolCall {
                id: "tc0".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "read_file".to_string(),
                    arguments: r#"{"path":"a.rs"}"#.to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        },
        Message {
            role: "tool".to_string(),
            content: Some("file contents".into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("tc0".to_string()),
        },
    ];
    let o = step_check_structured_plan(FinalPlanGateArgs {
        msg: &ok,
        messages: &hist,
        cfg: &cfg,
        workspace_is_set: false,
        gate_context: build_final_plan_gate_context(
            FinalPlanRequirementMode::WorkflowReflection,
            PlanRequirementSource::WorkflowReflection,
        ),
        final_plan_require_strict_workflow_node_coverage: false,
        final_plan_semantic_check_enabled: true,
        final_plan_semantic_check_max_non_readonly_tools: 4,
        layer_need: None,
        validate_only_binding_ids: None,
        plan_rewrite_attempts: 0,
        plan_rewrite_max_attempts: 2,
    });
    assert_eq!(o.route, FinalPlanGateRoute::PendingSemanticConsistencyLlm);
    assert_eq!(
        o.decision_reason,
        FinalPlanGateDecisionReason::PendingSemanticConsistencyLlm
    );
    assert!(matches!(
        o.after,
        AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm { .. }
    ));
}

#[test]
fn semantic_completed_consistent_stops() {
    let o = run_final_plan_gate_semantic_completed(
        &PlanSemanticLlmOutcome {
            consistent: true,
            violation_codes: vec![],
            rationale: None,
            user_cancelled: false,
        },
        0,
        3,
    );
    assert_eq!(o.route, FinalPlanGateRoute::SemanticConsistencyAcceptedStop);
    assert!(matches!(o.after, AfterFinalAssistant::StopTurn));
    assert_eq!(o.next_plan_rewrite_count, None);
}

#[test]
fn semantic_completed_inconsistent_rewrites() {
    let o = run_final_plan_gate_semantic_completed(
        &PlanSemanticLlmOutcome {
            consistent: false,
            violation_codes: vec!["x".into()],
            rationale: Some("r".into()),
            user_cancelled: false,
        },
        1,
        3,
    );
    assert_eq!(o.route, FinalPlanGateRoute::SemanticMismatchRequestRewrite);
    assert!(matches!(
        o.after,
        AfterFinalAssistant::RequestPlanRewrite(_)
    ));
    assert_eq!(o.next_plan_rewrite_count, Some(2));
}

#[test]
fn semantic_completed_inconsistent_exhausted() {
    let o = run_final_plan_gate_semantic_completed(
        &PlanSemanticLlmOutcome {
            consistent: false,
            violation_codes: vec!["x".into()],
            rationale: None,
            user_cancelled: false,
        },
        3,
        3,
    );
    assert_eq!(
        o.route,
        FinalPlanGateRoute::SemanticMismatchRewriteExhausted
    );
    assert!(matches!(
        o.after,
        AfterFinalAssistant::StopTurnPlanRewriteExhausted {
            reason: PlanRewriteExhaustedReason::PlanSemanticInconsistent
        }
    ));
}
