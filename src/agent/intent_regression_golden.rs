//! 意图识别金样回归：`fixtures/intent_regression.jsonl`。

use crate::agent::intent_pipeline::{IntentAction, IntentContext, assess_and_route};
use crate::agent::intent_router::{ExecuteIntentThresholds, IntentKind};
use serde::Deserialize;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    task: String,
    #[serde(default)]
    ctx: GoldenCtx,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize, Default)]
struct GoldenCtx {
    #[serde(default)]
    in_clarification_flow: bool,
    #[serde(default)]
    recent_user_messages: Vec<String>,
    #[serde(default)]
    has_recent_tool_failure: bool,
    #[serde(default)]
    l0_routing_boost_enabled: bool,
    #[serde(default)]
    l2_min_confidence: Option<f32>,
    #[serde(default)]
    thresholds: Option<GoldenThresholds>,
}

#[derive(Debug, Deserialize)]
struct GoldenThresholds {
    low: f32,
    high: f32,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    kind: String,
    primary_intent: String,
    action: String,
    #[serde(default)]
    need_clarification: bool,
    #[serde(default)]
    abstain: bool,
    #[serde(default)]
    secondary_intents_contains: Vec<String>,
    #[serde(default)]
    secondary_intents_excludes: Vec<String>,
}

fn build_context(c: GoldenCtx) -> IntentContext {
    let mut ctx = IntentContext {
        in_clarification_flow: c.in_clarification_flow,
        recent_user_messages: c.recent_user_messages,
        has_recent_tool_failure: c.has_recent_tool_failure,
        l0_routing_boost_enabled: c.l0_routing_boost_enabled,
        ..Default::default()
    };
    if let Some(v) = c.l2_min_confidence {
        ctx.l2_min_confidence = v;
    }
    if let Some(t) = c.thresholds {
        ctx.thresholds = ExecuteIntentThresholds {
            low: t.low,
            high: t.high,
        };
    }
    ctx
}

fn parse_kind(s: &str) -> IntentKind {
    match s {
        "Greeting" => IntentKind::Greeting,
        "Qa" => IntentKind::Qa,
        "Execute" => IntentKind::Execute,
        "Ambiguous" => IntentKind::Ambiguous,
        other => panic!("unknown IntentKind label: {other}"),
    }
}

fn action_tag(a: &IntentAction) -> &'static str {
    match a {
        IntentAction::Execute => "Execute",
        IntentAction::DirectReply(_) => "DirectReply",
        IntentAction::ClarifyThenExecute(_) => "ClarifyThenExecute",
        IntentAction::ConfirmThenExecute(_) => "ConfirmThenExecute",
    }
}

#[test]
fn golden_intent_regression_lines_match_pipeline() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/intent_regression.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ctx = build_context(row.ctx);
        let got = assess_and_route(&row.task, &ctx);
        let want_kind = parse_kind(&row.expect.kind);
        assert_eq!(
            got.kind,
            want_kind,
            "{}:{} ({}): kind mismatch for task {:?}",
            path.display(),
            line_no + 1,
            row.id,
            row.task
        );
        assert_eq!(
            got.primary_intent,
            row.expect.primary_intent,
            "{}:{} ({}): primary_intent mismatch",
            path.display(),
            line_no + 1,
            row.id
        );
        assert_eq!(
            action_tag(&got.action),
            row.expect.action,
            "{}:{} ({}): action variant mismatch",
            path.display(),
            line_no + 1,
            row.id
        );
        assert_eq!(
            got.need_clarification,
            row.expect.need_clarification,
            "{}:{} ({}): need_clarification mismatch",
            path.display(),
            line_no + 1,
            row.id
        );
        assert_eq!(
            got.abstain,
            row.expect.abstain,
            "{}:{} ({}): abstain mismatch",
            path.display(),
            line_no + 1,
            row.id
        );

        if !row.expect.secondary_intents_contains.is_empty() {
            let got_set: HashSet<_> = got.secondary_intents.iter().collect();
            for s in &row.expect.secondary_intents_contains {
                assert!(
                    got_set.contains(s),
                    "{}:{} ({}): secondary_intents missing {s:?}; got {:?}",
                    path.display(),
                    line_no + 1,
                    row.id,
                    got.secondary_intents
                );
            }
        }
        for s in &row.expect.secondary_intents_excludes {
            assert!(
                !got.secondary_intents.iter().any(|x| x == s),
                "{}:{} ({}): secondary_intents must not contain {s:?}; got {:?}",
                path.display(),
                line_no + 1,
                row.id,
                got.secondary_intents
            );
        }
    }
}
