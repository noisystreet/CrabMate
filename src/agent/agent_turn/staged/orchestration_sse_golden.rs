//! 编排 FSM 相位与 SSE 控制面契约金样：`fixtures/orchestration_sse_golden.jsonl`。
//!
//! 与 [`super::fsm_orchestrator_golden`]、`fixtures/sse_control_golden.jsonl` 互补：
//! 前者验证 FSM 映射，后者验证分类器；本文件绑定「相位 → 典型 SSE 载荷 → handled/stop」。

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::turn_orchestrator_fsm::StagedTurnOrchestratorPhase;

#[derive(Debug, Deserialize)]
struct OrchestrationSseGoldenLine {
    id: String,
    fsm_phase: String,
    #[serde(default)]
    fsm_golden_id: Option<String>,
    sse_key: String,
    control: String,
    payload: Value,
}

fn line_ctx(path: &Path, line_no: usize, id: &str) -> String {
    format!("{}:{} ({})", path.display(), line_no + 1, id)
}

fn load_fsm_golden_ids(root: &Path) -> HashSet<String> {
    let path = root.join("fixtures/fsm_orchestrator_golden.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut ids = HashSet::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let v: Value = serde_json::from_str(t).expect("fsm golden json");
        if let Some(id) = v.get("id").and_then(|x| x.as_str()) {
            ids.insert(id.to_string());
        }
    }
    ids
}

fn assert_fsm_phase_label(ctx: &str, label: &str) {
    let _phase = match label {
        "pre_plan" => StagedTurnOrchestratorPhase::PrePlan,
        "plan_ready" => StagedTurnOrchestratorPhase::PlanReady,
        "step_running" => StagedTurnOrchestratorPhase::StepRunning,
        "done" => StagedTurnOrchestratorPhase::Done,
        "degraded_to_outer_loop" => StagedTurnOrchestratorPhase::DegradedToOuterLoop,
        other => panic!("{ctx}: unknown fsm_phase label {other}"),
    };
}

#[test]
fn golden_orchestration_sse_control_matches_fsm_contract() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/orchestration_sse_golden.jsonl");
    let fsm_ids = load_fsm_golden_ids(&root);
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: OrchestrationSseGoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ctx = line_ctx(path.as_path(), line_no, &row.id);

        assert_fsm_phase_label(&ctx, &row.fsm_phase);

        if let Some(ref fid) = row.fsm_golden_id {
            assert!(
                fsm_ids.contains(fid),
                "{ctx}: fsm_golden_id {fid} not in fsm_orchestrator_golden.jsonl"
            );
        }

        let obj = row
            .payload
            .as_object()
            .unwrap_or_else(|| panic!("{ctx}: payload must be object"));
        assert!(
            crabmate_sse_protocol::key_present_non_null(obj, &row.sse_key),
            "{ctx}: payload missing sse_key {}",
            row.sse_key
        );

        let got = crabmate_sse_protocol::classify_sse_control_outcome(&row.payload);
        assert_eq!(
            got,
            row.control.as_str(),
            "{ctx}: classify expected {} got {}",
            row.control,
            got
        );
    }
}
