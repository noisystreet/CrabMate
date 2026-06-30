//! 验收规则金样回归：`fixtures/acceptance_regression.jsonl`。

use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::acceptance::{
    AcceptanceEvidence, AcceptanceSpec, ExitCodePolicy, FileResolveKind, VerifyOutcome,
    default_exit_code_for_build_execution_description, effective_plan_step_acceptance,
    parse_exit_code_from_combined_output, verify_against_spec,
};
use crate::plan_artifact::{PlanStepAcceptance, PlanStepExecutorKind, PlanStepV1};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    case: String,
    #[serde(flatten)]
    body: serde_json::Value,
}

fn line_ctx(path: &Path, line_no: usize, id: &str) -> String {
    format!("{}:{} ({})", path.display(), line_no + 1, id)
}

fn body_str<'a>(body: &'a serde_json::Value, key: &str, ctx: &str) -> &'a str {
    body[key]
        .as_str()
        .unwrap_or_else(|| panic!("{ctx}: missing {key}"))
}

fn executor_kind_from_label(label: &str) -> PlanStepExecutorKind {
    match label {
        "test_runner" => PlanStepExecutorKind::TestRunner,
        "review_readonly" => PlanStepExecutorKind::ReviewReadonly,
        other => panic!("unknown executor_kind: {other}"),
    }
}

fn assert_effective_acceptance(ctx: &str, body: &serde_json::Value) {
    let executor_kind = body_str(body, "executor_kind", ctx);
    let description = body_str(body, "description", ctx);
    let expect_some = body["expect_some"]
        .as_bool()
        .unwrap_or_else(|| panic!("{ctx}: missing expect_some"));
    let step = PlanStepV1 {
        id: "g".into(),
        description: description.to_string(),
        workflow_node_id: None,
        executor_kind: Some(executor_kind_from_label(executor_kind)),
        step_kind: None,
        acceptance: body
            .get("acceptance")
            .filter(|v| !v.is_null())
            .and_then(|v| serde_json::from_value::<PlanStepAcceptance>(v.clone()).ok()),
        max_step_retries: None,
        transitions: None,
    };
    let got = effective_plan_step_acceptance(&step);
    assert_eq!(got.is_some(), expect_some, "{ctx}: effective presence");
    if let Some(expect_code) = body.get("expect_exit_code").and_then(|v| v.as_i64()) {
        let eff = got.expect("effective");
        assert_eq!(
            eff.expect_exit_code,
            Some(expect_code as i32),
            "{ctx}: exit code"
        );
    }
}

fn assert_build_desc_exit(ctx: &str, body: &serde_json::Value) {
    let description = body_str(body, "description", ctx);
    let got = default_exit_code_for_build_execution_description(description);
    match body.get("expect_exit_code") {
        None | Some(serde_json::Value::Null) => assert!(got.is_none(), "{ctx}"),
        Some(v) => assert_eq!(got, Some(v.as_i64().expect("code") as i32), "{ctx}"),
    }
}

fn assert_exit_code_parse(ctx: &str, body: &serde_json::Value) {
    let output = body_str(body, "output", ctx);
    let got = parse_exit_code_from_combined_output(output);
    match body.get("expect") {
        None | Some(serde_json::Value::Null) => assert!(got.is_none(), "{ctx}"),
        Some(v) => assert_eq!(got, Some(v.as_i64().expect("code") as i32), "{ctx}"),
    }
}

fn spec_from_json(v: &serde_json::Value) -> AcceptanceSpec {
    let mut spec = AcceptanceSpec::default();
    if let Some(code) = v.get("expect_exit_code").and_then(|c| c.as_i64()) {
        spec.expect_exit_code = Some(code as i32);
    }
    if let Some(s) = v.get("expect_stdout_contains").and_then(|c| c.as_str()) {
        spec.expect_stdout_contains = Some(s.to_string());
    }
    if let Some(s) = v.get("expect_stderr_contains").and_then(|c| c.as_str()) {
        spec.expect_stderr_contains = Some(s.to_string());
    }
    if let Some(policy) = v.get("exit_code_policy").and_then(|c| c.as_str()) {
        spec.exit_code_policy = match policy {
            "lenient" => ExitCodePolicy::LenientIfUnparsed,
            _ => ExitCodePolicy::DefaultZeroIfMissing,
        };
    }
    spec
}

fn assert_verify(ctx: &str, body: &serde_json::Value) {
    let spec = spec_from_json(body.get("spec").expect("spec"));
    let ev_json = body.get("evidence").expect("evidence");
    let fallback_exit_code = ev_json
        .get("fallback_exit_code")
        .and_then(|c| c.as_i64())
        .map(|c| c as i32);
    let stdout = ev_json.get("stdout").and_then(|s| s.as_str()).unwrap_or("");
    let stderr = ev_json.get("stderr").and_then(|s| s.as_str()).unwrap_or("");
    let tool_output = ev_json
        .get("tool_output")
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let ev = AcceptanceEvidence {
        tool_name: "run_command",
        tool_output,
        stdout,
        stderr,
        tool_error: None,
        fallback_exit_code,
        workspace_root: std::path::Path::new("/tmp"),
        file_resolve: FileResolveKind::AbsolutizeRelative,
        combined_text_override: None,
    };
    let outcome = verify_against_spec(&spec, &ev);
    let expect = body_str(body, "expect", ctx);
    match expect {
        "pass" => assert_eq!(outcome, VerifyOutcome::Pass, "{ctx}"),
        "fail" => {
            let reason_contains = body
                .get("reason_contains")
                .and_then(|s| s.as_str())
                .unwrap_or("");
            match outcome {
                VerifyOutcome::Fail { reason } => {
                    assert!(
                        reason.contains(reason_contains),
                        "{ctx}: reason {reason:?} missing {reason_contains}"
                    );
                }
                VerifyOutcome::Pass => panic!("{ctx}: expected fail"),
            }
        }
        other => panic!("{ctx}: unknown expect {other}"),
    }
}

fn assert_golden_acceptance_line(ctx: &str, row: &GoldenLine) {
    match row.case.as_str() {
        "effective_acceptance" => assert_effective_acceptance(ctx, &row.body),
        "build_desc_exit" => assert_build_desc_exit(ctx, &row.body),
        "exit_code_parse" => assert_exit_code_parse(ctx, &row.body),
        "verify" => assert_verify(ctx, &row.body),
        other => panic!("{ctx}: unknown case {other}"),
    }
}

#[test]
fn golden_acceptance_regression_matches_rules() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = root.join("../../fixtures/acceptance_regression.jsonl");
    let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

    for (line_no, line) in raw.lines().enumerate() {
        let t = line.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(t).unwrap_or_else(|e| {
            panic!("{}:{}: invalid json: {e}\n{t}", path.display(), line_no + 1)
        });
        let ctx = line_ctx(path.as_path(), line_no, &row.id);
        assert_golden_acceptance_line(&ctx, &row);
    }
}
