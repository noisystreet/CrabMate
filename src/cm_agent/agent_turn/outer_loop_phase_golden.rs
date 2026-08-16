//! `fixtures/outer_loop_phase_golden.jsonl`：外循环相位记账 + reflect reduce（零 IO）。

use serde::Deserialize;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use super::outer_loop_driver::OuterLoopDriver;
use super::outer_loop_fsm::{OuterLoopIterationExit, OuterLoopIterationPhase, ReflectBranchCtl};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    steps: Vec<Value>,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    phase: String,
    #[serde(default)]
    last_exit: Option<String>,
    #[serde(default)]
    last_reflect: Option<String>,
    #[serde(default)]
    reflect_reduce: Option<String>,
}

fn phase_from(s: &str) -> OuterLoopIterationPhase {
    match s {
        "iteration_enter" => OuterLoopIterationPhase::IterationEnter,
        "prepare_context_done" => OuterLoopIterationPhase::PrepareContextDone,
        "after_planner_model" => OuterLoopIterationPhase::AfterPlannerModel,
        "reflect_decided" => OuterLoopIterationPhase::ReflectDecided,
        "tools_execute" => OuterLoopIterationPhase::ToolsExecute,
        other => panic!("unknown phase {other}"),
    }
}

fn reflect_from(s: &str) -> ReflectBranchCtl {
    match s {
        "break_outer" => ReflectBranchCtl::BreakOuter,
        "continue_outer" => ReflectBranchCtl::ContinueOuter,
        "proceed_to_tools" => ReflectBranchCtl::ProceedToTools,
        other => panic!("unknown reflect {other}"),
    }
}

fn exit_from(s: &str) -> OuterLoopIterationExit {
    match s {
        "continue_next_iteration" => OuterLoopIterationExit::ContinueNextIteration,
        "stop_outer_loop" => OuterLoopIterationExit::StopOuterLoop,
        other => panic!("unknown exit {other}"),
    }
}

fn apply_step(driver: &mut OuterLoopDriver, step: &Value, last_reduce: &mut Option<&'static str>) {
    if let Some(p) = step.get("record_phase").and_then(|v| v.as_str()) {
        driver.record_phase(phase_from(p));
        return;
    }
    if let Some(r) = step.get("record_reflect").and_then(|v| v.as_str()) {
        let action = driver.record_reflect_branch(reflect_from(r));
        *last_reduce = Some(action.as_str());
        return;
    }
    if let Some(e) = step.get("record_exit").and_then(|v| v.as_str()) {
        driver.record_iteration_exit(exit_from(e));
        return;
    }
    if let Some(early) = step.get("post_tools_early_stop").and_then(|v| v.as_bool()) {
        let exit = driver.decide_post_tools_exit(early);
        driver.record_iteration_exit(exit);
        return;
    }
    panic!("unknown step keys: {step}");
}

#[test]
fn golden_outer_loop_phase() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/outer_loop_phase_golden.jsonl");
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
        let mut driver = OuterLoopDriver::new();
        let mut last_reduce: Option<&'static str> = None;
        for step in &row.steps {
            apply_step(&mut driver, step, &mut last_reduce);
        }
        assert_eq!(driver.phase_str(), row.expect.phase.as_str(), "{ctx}");
        if let Some(exit) = &row.expect.last_exit {
            assert_eq!(
                driver.last_iteration_exit().map(|e| e.as_trace_str()),
                Some(exit.as_str()),
                "{ctx}"
            );
        }
        if let Some(r) = &row.expect.last_reflect {
            assert_eq!(
                driver.last_reflect_branch().map(|c| c.as_trace_str()),
                Some(r.as_str()),
                "{ctx}"
            );
        }
        if let Some(rr) = &row.expect.reflect_reduce {
            assert_eq!(last_reduce, Some(rr.as_str()), "{ctx}");
        }
    }
}
