//! `fixtures/workflow_to_plan_requirement_golden.jsonl`：workflow 反思 → Gate 衔接（零 IO）。

use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use super::{FinalPlanRequirementMode, PerCoordinator, PerCoordinatorInit};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    policy: String,
    #[serde(default)]
    steps: Vec<Value>,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    source: String,
    require_plan: bool,
    require_plan_reason: String,
    gate_phase: String,
}

fn policy_from(s: &str) -> FinalPlanRequirementMode {
    match s {
        "never" => FinalPlanRequirementMode::Never,
        "always" => FinalPlanRequirementMode::Always,
        "workflow_reflection" => FinalPlanRequirementMode::WorkflowReflection,
        other => panic!("unknown policy {other}"),
    }
}

fn pc(policy: FinalPlanRequirementMode) -> PerCoordinator {
    PerCoordinator::new(PerCoordinatorInit {
        reflection_default_max_rounds: 5,
        final_plan_policy: policy,
        plan_rewrite_max_attempts: 2,
        final_plan_require_strict_workflow_node_coverage: false,
        final_plan_semantic_check_enabled: false,
        final_plan_semantic_check_max_non_readonly_tools: 0,
    })
}

fn apply_step(c: &mut PerCoordinator, step: &Value, ctx: &str) {
    if let Some(args) = step.get("prepare_workflow").and_then(|v| v.as_str()) {
        let _ = c.prepare_workflow_execute(args);
        return;
    }
    if let Some(ty) = step.get("append_inject").and_then(|v| v.as_str()) {
        let inject = serde_json::json!({ "instruction_type": ty });
        let mut msgs = Vec::new();
        PerCoordinator::append_tool_result_and_reflection(
            c,
            &mut msgs,
            "tc-golden".to_string(),
            "ok".to_string(),
            Some(inject),
        );
        return;
    }
    panic!("{ctx}: unknown step {step}");
}

#[test]
fn golden_workflow_to_plan_requirement() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/workflow_to_plan_requirement_golden.jsonl");
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
        let mut c = pc(policy_from(&row.policy));
        for step in &row.steps {
            apply_step(&mut c, step, &ctx);
        }
        assert_eq!(
            c.plan_requirement_source_label(),
            row.expect.source.as_str(),
            "{ctx}"
        );
        assert_eq!(
            c.require_plan_in_final_flag_snapshot(),
            row.expect.require_plan,
            "{ctx}"
        );
        assert_eq!(
            c.require_plan_reason_label(),
            row.expect.require_plan_reason.as_str(),
            "{ctx}"
        );
        assert_eq!(
            c.gate_phase_label(),
            row.expect.gate_phase.as_str(),
            "{ctx}"
        );
    }
}
