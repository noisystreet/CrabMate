//! `TurnRouteDecision` v1 金样：`fixtures/turn_route_decision_golden.jsonl`。

use crabmate_agent::agent_turn::{
    HierarchicalTurnEntryResolution, IntentGateSnapshot, StagedGateSnapshot,
    StagedPlanningDenyReason, StagedPlanningGateOutcome, TurnRouteDecisionV1,
    build_hierarchical_turn_route_decision, build_non_hierarchical_intent_finished_early_decision,
    build_non_hierarchical_turn_route_decision, intent_gate_snapshot_from_decision,
    resolve_hierarchical_post_intent_route, resolve_non_hierarchical_turn,
    staged_gate_snapshot_from_outcome,
};
use crabmate_agent::intent_pipeline::{IntentAction, IntentDecision};
use crabmate_agent::intent_router::IntentKind;
use crabmate_config::PlannerExecutorMode;
use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    cfg_mode: String,
    intent_gate: Value,
    #[serde(default)]
    staged_gate: Option<Value>,
    #[serde(default)]
    expect_early: bool,
    #[serde(default)]
    expect_hierarchical: bool,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    top_level: String,
    orchestration_mode: String,
    #[serde(default)]
    turn_phase: Option<String>,
    #[serde(default)]
    freeform_because: Option<Value>,
    #[serde(default)]
    hierarchical_post_intent_route: Option<String>,
    #[serde(default)]
    staged_gate: Option<String>,
}

fn cfg_with(mode: &str) -> crabmate_config::AgentConfig {
    let pem = PlannerExecutorMode::parse(mode).expect("planner mode");
    let mut c = crabmate_config::load_config(None).expect("embed default config");
    c.per_plan_policy.planner_executor_mode = pem;
    c
}

fn parse_intent_gate(v: &Value) -> IntentGateSnapshot {
    let outcome = v["outcome"].as_str().expect("intent_gate.outcome");
    match outcome {
        "disabled" => IntentGateSnapshot::Disabled,
        "empty_task" => IntentGateSnapshot::EmptyTask,
        "finished_early" => IntentGateSnapshot::FinishedEarly {
            kind: v.get("kind").and_then(|x| x.as_str()).map(str::to_string),
            primary_intent: v
                .get("primary_intent")
                .and_then(|x| x.as_str())
                .map(str::to_string),
            action: v.get("action").and_then(|x| x.as_str()).map(str::to_string),
        },
        "proceed_execute" => IntentGateSnapshot::ProceedExecute {
            kind: v["kind"].as_str().expect("kind").to_string(),
            primary_intent: v["primary_intent"].as_str().expect("primary").to_string(),
            action: v["action"].as_str().expect("action").to_string(),
            confidence: v["confidence"].as_f64().expect("confidence") as f32,
            need_clarification: v
                .get("need_clarification")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
        },
        other => panic!("unknown intent_gate outcome {other}"),
    }
}

fn parse_staged_gate(v: &Value) -> StagedPlanningGateOutcome {
    match v["outcome"].as_str().expect("staged_gate.outcome") {
        "allow" => {
            let primary = v["primary_intent"].as_str().expect("primary").to_string();
            let confidence = v["confidence"].as_f64().expect("confidence") as f32;
            StagedPlanningGateOutcome::Allow {
                task_preview: primary.clone(),
                intent_kind: IntentKind::Execute,
                primary_intent: primary.clone(),
                confidence,
                decision: IntentDecision {
                    kind: IntentKind::Execute,
                    primary_intent: primary,
                    secondary_intents: Vec::new(),
                    confidence,
                    abstain: false,
                    need_clarification: false,
                    action: IntentAction::Execute,
                },
            }
        }
        "deny" => {
            let reason = match v["reason"].as_str().expect("reason") {
                "empty_effective_task" => StagedPlanningDenyReason::EmptyEffectiveTask,
                "intent_pipeline_not_execute" => StagedPlanningDenyReason::IntentPipelineNotExecute,
                "advisory_execute_bypass_staged" => {
                    StagedPlanningDenyReason::AdvisoryExecuteBypassStaged
                }
                "readonly_overview_bypass_staged" => {
                    StagedPlanningDenyReason::ReadonlyOverviewBypassStaged
                }
                other => panic!("unknown deny reason {other}"),
            };
            StagedPlanningGateOutcome::Deny {
                reason,
                task_preview: None,
                intent_decision: None,
            }
        }
        other => panic!("unknown staged_gate outcome {other}"),
    }
}

fn decision_from_intent_gate(snapshot: &IntentGateSnapshot) -> IntentDecision {
    match snapshot {
        IntentGateSnapshot::ProceedExecute {
            kind,
            primary_intent,
            action,
            confidence,
            need_clarification,
            ..
        } => IntentDecision {
            kind: match kind.as_str() {
                "execute" => IntentKind::Execute,
                "greeting" => IntentKind::Greeting,
                "qa" => IntentKind::Qa,
                _ => IntentKind::Ambiguous,
            },
            primary_intent: primary_intent.clone(),
            secondary_intents: Vec::new(),
            confidence: *confidence,
            abstain: false,
            need_clarification: *need_clarification,
            action: match action.as_str() {
                "execute" => IntentAction::Execute,
                "direct_reply" => IntentAction::DirectReply(String::new()),
                "clarify_then_execute" => IntentAction::ClarifyThenExecute(String::new()),
                "confirm_then_execute" => IntentAction::ConfirmThenExecute(String::new()),
                other => panic!("unknown action {other}"),
            },
        },
        _ => panic!("need ProceedExecute for hierarchical resolution"),
    }
}

fn assert_freeform_because(decision: &TurnRouteDecisionV1, expect: &Value, ctx: &str) {
    match expect {
        Value::Null => assert!(decision.freeform_because.is_none(), "{ctx}"),
        Value::String(s) => {
            assert_eq!(
                decision.freeform_because.as_deref(),
                Some(s.as_str()),
                "{ctx}"
            )
        }
        other => panic!("{ctx}: unexpected freeform_because expect {other}"),
    }
}

#[test]
fn golden_turn_route_decision() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/turn_route_decision_golden.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ctx = format!("{}:{} ({})", path.display(), line_no + 1, row.id);
        let cfg = cfg_with(&row.cfg_mode);
        let intent_gate = parse_intent_gate(&row.intent_gate);

        let decision = if row.expect_early {
            build_non_hierarchical_intent_finished_early_decision(&cfg, intent_gate)
        } else if row.expect_hierarchical {
            let assessment = decision_from_intent_gate(&intent_gate);
            let entry = resolve_hierarchical_post_intent_route(&assessment);
            let orchestration_mode =
                HierarchicalTurnEntryResolution::resolve(&assessment).orchestration_mode;
            build_hierarchical_turn_route_decision(
                &cfg,
                intent_gate_snapshot_from_decision(&assessment),
                orchestration_mode,
                entry,
            )
        } else {
            let staged = parse_staged_gate(row.staged_gate.as_ref().expect("staged_gate"));
            let entry = resolve_non_hierarchical_turn(&cfg, &staged);
            build_non_hierarchical_turn_route_decision(
                &cfg,
                intent_gate,
                staged_gate_snapshot_from_outcome(&staged),
                &entry,
            )
        };

        assert_eq!(decision.version, 1, "{ctx}");
        assert_eq!(decision.top_level, row.expect.top_level, "{ctx}");
        assert_eq!(
            decision.orchestration_mode, row.expect.orchestration_mode,
            "{ctx}"
        );
        if let Some(phase) = &row.expect.turn_phase {
            assert_eq!(decision.turn_phase, *phase, "{ctx}");
        }
        assert_freeform_because(
            &decision,
            &row.expect.freeform_because.clone().unwrap_or(Value::Null),
            &ctx,
        );
        if let Some(route) = &row.expect.hierarchical_post_intent_route {
            assert_eq!(
                decision.hierarchical_post_intent_route.as_deref(),
                Some(route.as_str()),
                "{ctx}"
            );
        }
        if let Some(sg) = &row.expect.staged_gate {
            assert!(
                matches!(
                    (&decision.staged_gate, sg.as_str()),
                    (StagedGateSnapshot::NotEvaluated, "not_evaluated")
                ),
                "{ctx}"
            );
        }
        assert!(decision.to_json().expect("json").contains("\"version\":1"));
    }
}
