//! `TurnRouteDecision` v1 金样：`fixtures/turn_route_decision_golden.jsonl`（经 [`assess_turn_routing`]）。

use crate::agent::agent_turn::policy::{
    AssessTurnRoutingParams, TurnRouteDecisionV1, TurnRouteDriver, TurnStartSnapshot,
    assess_turn_routing,
};
use crate::test_fixtures::{for_each_jsonl_record, line_ctx};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    turn_start: Value,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    orchestration_mode: String,
    #[serde(default, alias = "freeform_because")]
    react_because: Option<Value>,
    #[serde(default)]
    driver: Option<String>,
}

fn default_cfg() -> crate::cm_config::AgentConfig {
    crate::cm_config::load_config(None).expect("embed default config")
}

fn parse_turn_start(v: &Value) -> TurnStartSnapshot {
    let outcome = v["outcome"].as_str().expect("turn_start.outcome");
    match outcome {
        "disabled" => TurnStartSnapshot::Disabled,
        "empty_task" => TurnStartSnapshot::EmptyTask,
        "act_heuristics" => TurnStartSnapshot::ActHeuristics {
            review_readonly: v
                .get("review_readonly")
                .and_then(|x| x.as_bool())
                .unwrap_or(false),
        },
        other => panic!("unknown turn_start outcome {other}"),
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
    for_each_jsonl_record::<GoldenLine, _>(
        "fixtures/turn_route_decision_golden.jsonl",
        |path, line_no, row| {
            let ctx = line_ctx(path, line_no, &row.id);
            let cfg = default_cfg();
            let turn_start = parse_turn_start(&row.turn_start);

            let assessed = assess_turn_routing(AssessTurnRoutingParams {
                cfg: &cfg,
                turn_start: turn_start.clone(),
            });
            let decision = &assessed.decision;

            assert_eq!(decision.version, 1, "{ctx}");
            assert_eq!(
                decision.orchestration_mode, row.expect.orchestration_mode,
                "{ctx}"
            );
            assert_freeform_because(
                decision,
                &row.expect.react_because.clone().unwrap_or(Value::Null),
                &ctx,
            );
            if let Some(driver) = &row.expect.driver {
                match driver.as_str() {
                    "react" => assert!(matches!(assessed.driver, TurnRouteDriver::ReAct), "{ctx}"),
                    other => panic!("{ctx}: unknown driver expect {other}"),
                }
            }
            assert!(decision.to_json().expect("json").contains("\"version\":1"));
        },
    );
}
