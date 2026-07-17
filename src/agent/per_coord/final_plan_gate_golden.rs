//! 终答 Gate 金样回归：`fixtures/final_plan_gate_golden.jsonl`。

use crate::agent::per_plan_semantic_check::PlanSemanticLlmOutcome;
use crate::agent::reflection::plan_rewrite::PlanRewriteExhaustedReason;
use crate::config::AgentConfig;
use crate::types::{Message, MessageContent};

use super::final_plan_gate::{
    FinalPlanGateArgs, FinalPlanGateEvent, FinalPlanGatePhase, FinalPlanGateRoute,
    run_final_plan_gate, run_final_plan_gate_semantic_completed, step_check_structured_plan,
};
use super::final_plan_gate_context::{FinalPlanRequirePlanReason, build_final_plan_gate_context};
use super::{AfterFinalAssistant, FinalPlanRequirementMode, PlanRequirementSource};

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

fn minimal_cfg() -> AgentConfig {
    crate::config::load_config(None).expect("embedded default config must load")
}

fn policy_from_label(label: &str) -> FinalPlanRequirementMode {
    match label {
        "never" => FinalPlanRequirementMode::Never,
        "workflow_reflection" => FinalPlanRequirementMode::WorkflowReflection,
        "always" => FinalPlanRequirementMode::Always,
        other => panic!("unknown policy label: {other}"),
    }
}

fn source_from_label(label: &str) -> PlanRequirementSource {
    match label {
        "none" => PlanRequirementSource::None,
        "workflow_reflection" => PlanRequirementSource::WorkflowReflection,
        "config_always" => PlanRequirementSource::ConfigAlways,
        other => panic!("unknown source label: {other}"),
    }
}

fn phase_from_label(label: &str) -> FinalPlanGatePhase {
    match label {
        "no_requirement" => FinalPlanGatePhase::NoRequirement,
        "check_structured_plan" => FinalPlanGatePhase::CheckStructuredPlan,
        "pending_semantic_llm" => FinalPlanGatePhase::PendingSemanticLlm,
        other => panic!("unknown phase label: {other}"),
    }
}

fn phase_label(phase: FinalPlanGatePhase) -> &'static str {
    match phase {
        FinalPlanGatePhase::NoRequirement => "no_requirement",
        FinalPlanGatePhase::CheckStructuredPlan => "check_structured_plan",
        FinalPlanGatePhase::PendingSemanticLlm => "pending_semantic_llm",
    }
}

fn route_label(route: FinalPlanGateRoute) -> &'static str {
    match route {
        FinalPlanGateRoute::StopNoRequirement => "stop_no_requirement",
        FinalPlanGateRoute::AcceptStructuredPlanOk => "accept_structured_plan_ok",
        FinalPlanGateRoute::PendingSemanticConsistencyLlm => "pending_semantic_consistency_llm",
        FinalPlanGateRoute::SemanticsFailedRequestRewrite => "semantics_failed_request_rewrite",
        FinalPlanGateRoute::SemanticsFailedRewriteExhausted => "semantics_failed_rewrite_exhausted",
        FinalPlanGateRoute::SemanticConsistencyAcceptedStop => "semantic_consistency_accepted_stop",
        FinalPlanGateRoute::SemanticMismatchRequestRewrite => "semantic_mismatch_request_rewrite",
        FinalPlanGateRoute::SemanticMismatchRewriteExhausted => {
            "semantic_mismatch_rewrite_exhausted"
        }
    }
}

fn after_label(after: &AfterFinalAssistant) -> &'static str {
    match after {
        AfterFinalAssistant::StopTurn => "stop_turn",
        AfterFinalAssistant::RequestPlanRewrite(_) => "request_plan_rewrite",
        AfterFinalAssistant::StopTurnPlanRewriteExhausted { .. } => {
            "stop_turn_plan_rewrite_exhausted"
        }
        AfterFinalAssistant::StopTurnPendingPlanConsistencyLlm { .. } => {
            "stop_turn_pending_plan_consistency_llm"
        }
    }
}

fn exhausted_reason_label(reason: PlanRewriteExhaustedReason) -> &'static str {
    reason.as_str()
}

fn assistant_from_label(label: &str) -> Message {
    match label {
        "valid_plan" => Message {
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
        },
        "no_plan" => Message {
            role: "assistant".to_string(),
            content: Some(MessageContent::Text("no json plan".to_string())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        },
        other => panic!("unknown assistant label: {other}"),
    }
}

fn gate_args<'a>(
    msg: &'a Message,
    policy: FinalPlanRequirementMode,
    source: PlanRequirementSource,
    attempts: usize,
    max_attempts: usize,
    cfg: &'a AgentConfig,
) -> FinalPlanGateArgs<'a> {
    FinalPlanGateArgs {
        msg,
        messages: &[],
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

fn assert_resolve_phase(ctx: &str, body: &serde_json::Value) {
    let policy = policy_from_label(body_str(body, "policy", ctx));
    let source = source_from_label(body_str(body, "source", ctx));
    let expect = body_str(body, "expect_phase", ctx);
    assert_eq!(
        phase_label(build_final_plan_gate_context(policy, source).phase),
        expect,
        "{ctx}: resolve_phase"
    );
}

fn assert_run_gate(ctx: &str, body: &serde_json::Value) {
    let cfg = minimal_cfg();
    let phase = phase_from_label(body_str(body, "phase", ctx));
    let msg = Message::assistant_only("x".to_string());
    let o = run_final_plan_gate(
        phase,
        FinalPlanGateEvent::FinalAssistantArrived,
        gate_args(
            &msg,
            FinalPlanRequirementMode::Never,
            PlanRequirementSource::None,
            0,
            2,
            &cfg,
        ),
    );
    assert_eq!(
        route_label(o.route),
        body_str(body, "expect_route", ctx),
        "{ctx}"
    );
    assert_eq!(
        o.decision_reason.as_str(),
        body_str(body, "expect_reason", ctx),
        "{ctx}: reason"
    );
    assert_eq!(
        after_label(&o.after),
        body_str(body, "expect_after", ctx),
        "{ctx}: after"
    );
}

fn assert_step_check(ctx: &str, body: &serde_json::Value) {
    let cfg = minimal_cfg();
    let msg = assistant_from_label(body_str(body, "assistant", ctx));
    let policy = policy_from_label(body_str(body, "policy", ctx));
    let source = source_from_label(body_str(body, "source", ctx));
    let attempts = body["attempts"].as_u64().unwrap_or(0) as usize;
    let max_attempts = body["max_attempts"].as_u64().unwrap_or(2) as usize;
    let o = step_check_structured_plan(gate_args(
        &msg,
        policy,
        source,
        attempts,
        max_attempts,
        &cfg,
    ));
    assert_eq!(
        route_label(o.route),
        body_str(body, "expect_route", ctx),
        "{ctx}"
    );
    assert_eq!(
        o.decision_reason.as_str(),
        body_str(body, "expect_reason", ctx),
        "{ctx}: reason"
    );
    assert_eq!(
        after_label(&o.after),
        body_str(body, "expect_after", ctx),
        "{ctx}: after"
    );
    if let Some(expect_next) = body.get("expect_next_rewrite").and_then(|v| v.as_u64()) {
        assert_eq!(
            o.next_plan_rewrite_count,
            Some(expect_next as usize),
            "{ctx}: next_rewrite"
        );
    }
    if let Some(reason_label) = body.get("exhausted_reason").and_then(|v| v.as_str()) {
        match &o.after {
            AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason } => {
                assert_eq!(
                    exhausted_reason_label(*reason),
                    reason_label,
                    "{ctx}: exhausted_reason"
                );
            }
            other => panic!("{ctx}: expected exhausted after, got {other:?}"),
        }
    }
}

fn assert_semantic_completed(ctx: &str, body: &serde_json::Value) {
    let consistent = body["consistent"].as_bool().unwrap_or(true);
    let attempts = body["attempts"].as_u64().unwrap_or(0) as usize;
    let max_attempts = body["max_attempts"].as_u64().unwrap_or(3) as usize;
    let o = run_final_plan_gate_semantic_completed(
        &PlanSemanticLlmOutcome {
            consistent,
            violation_codes: if consistent { vec![] } else { vec!["x".into()] },
            rationale: None,
            user_cancelled: false,
        },
        attempts,
        max_attempts,
    );
    assert_eq!(
        route_label(o.route),
        body_str(body, "expect_route", ctx),
        "{ctx}"
    );
    assert_eq!(
        o.decision_reason.as_str(),
        body_str(body, "expect_reason", ctx),
        "{ctx}: reason"
    );
    assert_eq!(
        after_label(&o.after),
        body_str(body, "expect_after", ctx),
        "{ctx}: after"
    );
    if let Some(expect_next) = body.get("expect_next_rewrite").and_then(|v| v.as_u64()) {
        assert_eq!(
            o.next_plan_rewrite_count,
            Some(expect_next as usize),
            "{ctx}: next_rewrite"
        );
    }
    if let Some(reason_label) = body.get("exhausted_reason").and_then(|v| v.as_str()) {
        match &o.after {
            AfterFinalAssistant::StopTurnPlanRewriteExhausted { reason } => {
                assert_eq!(
                    exhausted_reason_label(*reason),
                    reason_label,
                    "{ctx}: exhausted_reason"
                );
            }
            other => panic!("{ctx}: expected exhausted after, got {other:?}"),
        }
    }
}

fn assert_gate_context(ctx: &str, body: &serde_json::Value) {
    let policy = policy_from_label(body_str(body, "policy", ctx));
    let source = source_from_label(body_str(body, "source", ctx));
    let gate_ctx = build_final_plan_gate_context(policy, source);
    let expect_require = body["expect_require_plan"].as_bool().unwrap();
    let expect_reason = body_str(body, "expect_reason", ctx);
    assert_eq!(gate_ctx.require_plan, expect_require, "{ctx}: require_plan");
    assert_eq!(
        match gate_ctx.require_plan_reason {
            FinalPlanRequirePlanReason::PolicyNever => "policy_never",
            FinalPlanRequirePlanReason::PolicyAlways => "policy_always",
            FinalPlanRequirePlanReason::WorkflowReflectionActive => {
                "workflow_reflection_active"
            }
            FinalPlanRequirePlanReason::NoActiveRequirement => "no_active_requirement",
        },
        expect_reason,
        "{ctx}: require_plan_reason"
    );
}

fn assert_golden_line(ctx: &str, row: &GoldenLine) {
    match row.case.as_str() {
        "resolve_phase" => assert_resolve_phase(ctx, &row.body),
        "gate_context" => assert_gate_context(ctx, &row.body),
        "run_gate" => assert_run_gate(ctx, &row.body),
        "step_check" => assert_step_check(ctx, &row.body),
        "semantic_completed" => assert_semantic_completed(ctx, &row.body),
        other => panic!("{ctx}: unknown case {other}"),
    }
}

#[test]
fn golden_final_plan_gate_lines_match_mappings() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/final_plan_gate_golden.jsonl");
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
        assert_golden_line(&ctx, &row);
    }
}
