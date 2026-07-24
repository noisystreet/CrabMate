//! `TurnRouteDecision` v1 金样：`fixtures/turn_route_decision_golden.jsonl`（经 [`assess_turn_routing`]）。

use crabmate_agent::agent_turn::{
    AssessTurnRoutingParams, IntentGateSnapshot, TurnRouteDecisionV1, TurnRouteDriver,
    TurnTopLevelDispatch, assess_turn_routing,
};
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
    expect_early: bool,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    top_level: String,
    orchestration_mode: String,
    #[serde(default)]
    turn_phase: Option<String>,
    #[serde(default, alias = "freeform_because")]
    react_because: Option<Value>,
    #[serde(default)]
    driver: Option<String>,
}

fn cfg_with(mode: &str) -> crabmate_config::AgentConfig {
    use crabmate_config::PlannerExecutorMode;
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

        let assessed = assess_turn_routing(AssessTurnRoutingParams {
            cfg: &cfg,
            top_level: TurnTopLevelDispatch::NonHierarchical,
            intent_gate: intent_gate.clone(),
        });
        let decision = &assessed.decision;

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
            decision,
            &row.expect.react_because.clone().unwrap_or(Value::Null),
            &ctx,
        );
        if row.expect_early {
            assert_eq!(assessed.driver, TurnRouteDriver::IntentEarlyExit, "{ctx}");
        } else if let Some(driver) = &row.expect.driver {
            match driver.as_str() {
                "non_hierarchical_freeform" => assert!(
                    matches!(assessed.driver, TurnRouteDriver::NonHierarchical(_)),
                    "{ctx}"
                ),
                other => panic!("{ctx}: unknown driver expect {other}"),
            }
        }
        assert!(decision.to_json().expect("json").contains("\"version\":1"));
    }
}
