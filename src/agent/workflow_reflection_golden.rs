//! workflow_reflection 控制器金样回归：`fixtures/workflow_reflection_regression.jsonl`。

use serde::Deserialize;

use crate::agent::workflow_reflection_controller::WorkflowReflectionController;
use crate::test_fixtures::{for_each_jsonl_record, line_ctx};

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
    for_each_jsonl_record::<GoldenLine, _>(
        "fixtures/workflow_reflection_regression.jsonl",
        |path, line_no, row| {
            let ctx = line_ctx(path, line_no, &row.id);

            let mut c = WorkflowReflectionController::new(row.default_max_rounds);
            for call in &row.pre_calls {
                let _ = c.decide(call);
            }
            let got = c.decide(&row.call);

            assert_eq!(got.execute, row.expect.execute, "{ctx} execute mismatch");

            let got_inject = got
                .inject_instruction
                .as_ref()
                .and_then(|v| v.get("instruction_type"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            assert_eq!(
                got_inject, row.expect.inject_instruction_type,
                "{ctx} inject instruction_type mismatch"
            );

            let got_stop = got
                .stop_output
                .as_ref()
                .and_then(|v| v.get("instruction_type"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            assert_eq!(
                got_stop, row.expect.stop_instruction_type,
                "{ctx} stop instruction_type mismatch"
            );

            let got_validate_only = got
                .workflow_args_patch
                .as_ref()
                .and_then(|v| v.get("validate_only"))
                .and_then(|v| v.as_bool());
            assert_eq!(
                got_validate_only, row.expect.validate_only,
                "{ctx} validate_only mismatch"
            );

            if let Some(want_round) = row.expect.stage_round {
                assert_eq!(c.stage_round(), want_round, "{ctx} stage_round mismatch");
            }
        },
    );
}
