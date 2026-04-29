//! 规划产物解析金样回归：`fixtures/plan_artifact_regression.jsonl`。

use serde::Deserialize;
use std::fs;
use std::path::PathBuf;

use crate::agent::plan_artifact::{
    PlanArtifactError, parse_agent_reply_plan_v1_with_validate_only_binding_ids,
};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    content: String,
    binding_ids: Option<Vec<String>>,
    expect: GoldenExpect,
}

#[derive(Debug, Deserialize)]
struct GoldenExpect {
    ok: bool,
    error: Option<String>,
}

fn error_tag(e: &PlanArtifactError) -> &'static str {
    match e {
        PlanArtifactError::NotFound => "NotFound",
        PlanArtifactError::WrongType(_) => "WrongType",
        PlanArtifactError::WrongVersion(_) => "WrongVersion",
        PlanArtifactError::EmptySteps => "EmptySteps",
        PlanArtifactError::TooManySteps { .. } => "TooManySteps",
        PlanArtifactError::NoTaskWithNonEmptySteps => "NoTaskWithNonEmptySteps",
        PlanArtifactError::InvalidStep { .. } => "InvalidStep",
        PlanArtifactError::WorkflowNodesNotFullyCovered { .. } => "WorkflowNodesNotFullyCovered",
        PlanArtifactError::ValidateOnlyPlanNodeBindingMismatch { .. } => {
            "ValidateOnlyPlanNodeBindingMismatch"
        }
    }
}

#[test]
fn golden_plan_artifact_regression_matches_parser() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("fixtures/plan_artifact_regression.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ids = row.binding_ids.as_deref();
        let got = parse_agent_reply_plan_v1_with_validate_only_binding_ids(&row.content, ids);

        assert_eq!(
            got.is_ok(),
            row.expect.ok,
            "{}:{} ({}) ok mismatch",
            path.display(),
            line_no + 1,
            row.id
        );
        if let Some(want_err) = row.expect.error.as_deref() {
            let got_err = got.err().unwrap_or_else(|| {
                panic!(
                    "{}:{} ({}) expected error {want_err} but got Ok",
                    path.display(),
                    line_no + 1,
                    row.id
                )
            });
            assert_eq!(
                error_tag(&got_err),
                want_err,
                "{}:{} ({}) error kind mismatch",
                path.display(),
                line_no + 1,
                row.id
            );
        }
    }
}
