//! workflow_reflection 控制器金样回归：`fixtures/workflow_reflection_regression.jsonl`。

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

use crate::agent::workflow_reflection_controller::WorkflowReflectionController;

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    default_max_rounds: usize,
    #[serde(default)]
    pre_calls: Vec<String>,
    call: String,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    execute: bool,
    inject_instruction_type: Option<String>,
    stop_instruction_type: Option<String>,
    validate_only: Option<bool>,
    stage_round: Option<usize>,
}

#[test]
fn golden_workflow_reflection_regression_matches_controller() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/workflow_reflection_regression.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });

        let mut c = WorkflowReflectionController::new(row.default_max_rounds);
        for call in &row.pre_calls {
            let _ = c.decide(call);
        }
        let got = c.decide(&row.call);

        assert_eq!(
            got.execute,
            row.expect.execute,
            "{}:{} ({}) execute mismatch",
            path.display(),
            line_no + 1,
            row.id
        );

        let got_inject = got
            .inject_instruction
            .as_ref()
            .and_then(|v| v.get("instruction_type"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        assert_eq!(
            got_inject,
            row.expect.inject_instruction_type,
            "{}:{} ({}) inject instruction_type mismatch",
            path.display(),
            line_no + 1,
            row.id
        );

        let got_stop = got
            .stop_output
            .as_ref()
            .and_then(|v| v.get("instruction_type"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        assert_eq!(
            got_stop,
            row.expect.stop_instruction_type,
            "{}:{} ({}) stop instruction_type mismatch",
            path.display(),
            line_no + 1,
            row.id
        );

        let got_validate_only = got
            .workflow_args_patch
            .as_ref()
            .and_then(|v| v.get("validate_only"))
            .and_then(|v| v.as_bool());
        assert_eq!(
            got_validate_only,
            row.expect.validate_only,
            "{}:{} ({}) validate_only mismatch",
            path.display(),
            line_no + 1,
            row.id
        );

        if let Some(want_round) = row.expect.stage_round {
            assert_eq!(
                c.stage_round(),
                want_round,
                "{}:{} ({}) stage_round mismatch",
                path.display(),
                line_no + 1,
                row.id
            );
        }
    }
}
