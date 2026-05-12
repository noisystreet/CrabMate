//! 规划产物解析金样回归：`fixtures/plan_artifact_regression.jsonl`。
//!
//! - **默认**（无 `mode` 或 `mode=parse`）：[`parse_agent_reply_plan_v1_with_validate_only_binding_ids`]（多候选块、仅终态 `NotFound` / `Ok`）。
//! - **`mode=validate`**：对单行 JSON 反序列化后跑 [`validate_agent_reply_plan_v1_with_validate_only_binding_ids`]，断言具体 [`PlanArtifactError`]。
//! - **`mode=validate_bind`**：[`validate_plan_binds_workflow_validate_nodes`]，须提供非空 `binding_ids`（视作 validate-only 的 `nodes[].id` 列表）。
//! - **`mode=validate_cover`**：[`validate_plan_covers_all_workflow_node_ids`]，须提供非空 `binding_ids`（视作须被覆盖的全部 workflow 节点 id）。

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::plan_artifact::{
    AgentReplyPlanV1, PlanArtifactError, parse_agent_reply_plan_v1_with_validate_only_binding_ids,
    validate_agent_reply_plan_v1_with_validate_only_binding_ids,
    validate_plan_binds_workflow_validate_nodes, validate_plan_covers_all_workflow_node_ids,
};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    /// `parse`（默认）| `validate` | `validate_bind` | `validate_cover`
    #[serde(default)]
    mode: Option<String>,
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

fn assert_error_match(
    path: &Path,
    line_no: usize,
    id: &str,
    got: Result<(), PlanArtifactError>,
    expect_ok: bool,
    want_err: Option<&str>,
) {
    assert_eq!(
        got.is_ok(),
        expect_ok,
        "{}:{} ({}) ok mismatch",
        path.display(),
        line_no + 1,
        id
    );
    if let Some(want_err) = want_err {
        let got_err = got.err().unwrap_or_else(|| {
            panic!(
                "{}:{} ({}) expected error {want_err} but got Ok",
                path.display(),
                line_no + 1,
                id
            );
        });
        assert_eq!(
            error_tag(&got_err),
            want_err,
            "{}:{} ({}) error kind mismatch",
            path.display(),
            line_no + 1,
            id
        );
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
        let mode = row.mode.as_deref().unwrap_or("parse");

        match mode {
            "parse" => {
                let got =
                    parse_agent_reply_plan_v1_with_validate_only_binding_ids(&row.content, ids);
                assert_error_match(
                    &path,
                    line_no + 1,
                    &row.id,
                    got.map(|_| ()),
                    row.expect.ok,
                    row.expect.error.as_deref(),
                );
            }
            "validate" => {
                let plan: AgentReplyPlanV1 =
                    serde_json::from_str(&row.content).unwrap_or_else(|e| {
                        panic!(
                            "{}:{} ({}): validate mode requires strict JSON: {e}",
                            path.display(),
                            line_no + 1,
                            row.id
                        );
                    });
                let got = validate_agent_reply_plan_v1_with_validate_only_binding_ids(&plan, ids);
                assert_error_match(
                    &path,
                    line_no + 1,
                    &row.id,
                    got,
                    row.expect.ok,
                    row.expect.error.as_deref(),
                );
            }
            "validate_bind" => {
                let ids_vec = row.binding_ids.as_ref().unwrap_or_else(|| {
                    panic!(
                        "{}:{} ({}): validate_bind requires binding_ids",
                        path.display(),
                        line_no + 1,
                        row.id
                    );
                });
                assert!(
                    !ids_vec.is_empty(),
                    "{}:{} ({}): validate_bind requires non-empty binding_ids",
                    path.display(),
                    line_no + 1,
                    row.id
                );
                let plan: AgentReplyPlanV1 =
                    serde_json::from_str(&row.content).unwrap_or_else(|e| {
                        panic!(
                            "{}:{} ({}): validate_bind requires strict JSON: {e}",
                            path.display(),
                            line_no + 1,
                            row.id
                        );
                    });
                let got = validate_plan_binds_workflow_validate_nodes(&plan, ids_vec);
                assert_error_match(
                    &path,
                    line_no + 1,
                    &row.id,
                    got,
                    row.expect.ok,
                    row.expect.error.as_deref(),
                );
            }
            "validate_cover" => {
                let ids_vec = row.binding_ids.as_ref().unwrap_or_else(|| {
                    panic!(
                        "{}:{} ({}): validate_cover requires binding_ids",
                        path.display(),
                        line_no + 1,
                        row.id
                    );
                });
                assert!(
                    !ids_vec.is_empty(),
                    "{}:{} ({}): validate_cover requires non-empty binding_ids",
                    path.display(),
                    line_no + 1,
                    row.id
                );
                let plan: AgentReplyPlanV1 =
                    serde_json::from_str(&row.content).unwrap_or_else(|e| {
                        panic!(
                            "{}:{} ({}): validate_cover requires strict JSON: {e}",
                            path.display(),
                            line_no + 1,
                            row.id
                        );
                    });
                let got = validate_plan_covers_all_workflow_node_ids(&plan, ids_vec);
                assert_error_match(
                    &path,
                    line_no + 1,
                    &row.id,
                    got,
                    row.expect.ok,
                    row.expect.error.as_deref(),
                );
            }
            other => panic!(
                "{}:{} ({}): unknown mode {other:?} (use parse|validate|validate_bind|validate_cover)",
                path.display(),
                line_no + 1,
                row.id
            ),
        }
    }
}
