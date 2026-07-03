//! 完成判定金样回归：`fixtures/turn_completion_golden.jsonl`。

use super::turn_completion_decision::{
    RollingHorizonStopVia, TurnCompletionDecision, evaluate_turn_early_stop,
    evaluate_turn_redundant_tools, evaluate_turn_staged_rolling_horizon_early_stop,
    evaluate_turn_suppress_replanning,
};
use crate::agent::plan_artifact::{PlanStepAcceptance, PlanStepV1};
use crate::types::{FunctionCall, Message, ToolCall};
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    case: String,
    fixture: String,
    #[serde(default)]
    entered_from_step: bool,
    #[serde(default)]
    steps_fixture: Option<String>,
    #[serde(default)]
    tool_fixture: Option<String>,
    expect: String,
    #[serde(default)]
    deny_reason: Option<String>,
    #[serde(default)]
    via: Option<String>,
}

fn line_ctx(path: &Path, line_no: usize, id: &str) -> String {
    format!("{}:{} ({})", path.display(), line_no + 1, id)
}

fn msg(role: &str, text: &str) -> Message {
    Message {
        role: role.to_string(),
        content: Some(text.into()),
        reasoning_content: None,
        reasoning_details: None,
        tool_calls: None,
        name: None,
        tool_call_id: None,
    }
}

fn tool_env(name: &str, output: &str) -> Message {
    let parsed = crate::tool_result::parse_legacy_output(name, output);
    msg(
        "tool",
        &crate::tool_result::encode_tool_message_envelope_v1(
            name,
            name.to_string(),
            &parsed,
            output,
            None,
        ),
    )
}

fn fixture_messages(name: &str) -> Vec<Message> {
    match name {
        "readonly_satisfied" => vec![
            msg("user", "分析当前目录"),
            tool_env("list_tree", "list tree: ."),
            msg(
                "assistant",
                "当前目录包含三个压缩包，分析结果如下，总结完成。",
            ),
        ],
        "build_satisfied" => vec![
            msg("user", "编译 hpcg"),
            tool_env(
                "run_command",
                "命令：make\n退出码：0\n标准输出：\nBuilt target hpcg",
            ),
            msg("assistant", "HPCG 编译完成成功。"),
        ],
        "build_step_acceptance_pass" => vec![
            Message::user_only("编译 hello"),
            Message::user_staged_step_injection("### 分步 1/1\n- id: build\n- 描述: 构建"),
            tool_env(
                "run_command",
                "退出码：0\n标准输出：\n[100%] Built target hello\n",
            ),
            Message::assistant_only("构建步骤完成。"),
        ],
        other => panic!("unknown fixture: {other}"),
    }
}

fn fixture_steps(name: &str) -> Vec<PlanStepV1> {
    match name {
        "probe_only" => vec![PlanStepV1 {
            id: "verify".into(),
            description: "检查产物是否存在".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: Some("verify".into()),
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }],
        other => panic!("unknown steps_fixture: {other}"),
    }
}

fn fixture_tool_calls(name: &str) -> Vec<ToolCall> {
    match name {
        "list_dir_probe" => vec![ToolCall {
            id: "call_1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: "list_dir".into(),
                arguments: r#"{"path":"."}"#.into(),
            },
        }],
        other => panic!("unknown tool_fixture: {other}"),
    }
}

fn rolling_acceptance_for_fixture(fixture: &str) -> Option<PlanStepAcceptance> {
    if fixture == "build_step_acceptance_pass" {
        Some(PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: Some("Built target hello".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        })
    } else {
        None
    }
}

fn assert_expect_allow(ctx: &str, decision: TurnCompletionDecision, via: Option<&str>) {
    assert!(
        decision.is_allow(),
        "{ctx}: expected allow, got {decision:?}"
    );
    if let Some(v) = via {
        assert_eq!(
            decision.rolling_horizon_via().map(|x| match x {
                RollingHorizonStopVia::HeuristicEarlyStop => "heuristic_early_stop",
                RollingHorizonStopVia::StepAcceptancePass => "step_acceptance_pass",
                RollingHorizonStopVia::GoalEvidenceSatisfied => "goal_evidence_satisfied",
            }),
            Some(v),
            "{ctx}: rolling via"
        );
    }
}

fn assert_expect_deny(ctx: &str, decision: TurnCompletionDecision, deny_reason: Option<&str>) {
    assert!(
        !decision.is_allow(),
        "{ctx}: expected deny, got {decision:?}"
    );
    if let Some(reason) = deny_reason {
        assert_eq!(decision.deny_reason(), Some(reason), "{ctx}: deny reason");
    }
}

fn assert_golden_line(ctx: &str, row: &GoldenLine) {
    let messages = fixture_messages(&row.fixture);
    let decision = match row.case.as_str() {
        "early_stop" => evaluate_turn_early_stop(&messages),
        "suppress_replanning" => {
            let steps = fixture_steps(row.steps_fixture.as_deref().unwrap_or("probe_only"));
            evaluate_turn_suppress_replanning(&messages, row.entered_from_step, &steps)
        }
        "redundant_tools" => {
            let tools = fixture_tool_calls(row.tool_fixture.as_deref().unwrap_or("list_dir_probe"));
            evaluate_turn_redundant_tools(&tools, &messages)
        }
        "rolling_horizon" => evaluate_turn_staged_rolling_horizon_early_stop(
            &messages,
            rolling_acceptance_for_fixture(&row.fixture).as_ref(),
            std::path::Path::new("/tmp"),
        ),
        other => panic!("{ctx}: unknown case {other}"),
    };
    match row.expect.as_str() {
        "allow" => assert_expect_allow(ctx, decision, row.via.as_deref()),
        "deny" => assert_expect_deny(ctx, decision, row.deny_reason.as_deref()),
        other => panic!("{ctx}: unknown expect {other}"),
    }
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn golden_turn_completion_lines_match_decisions() {
    let path = repo_root().join("fixtures/turn_completion_golden.jsonl");
    let text = fs::read_to_string(&path).expect("read golden");
    for (line_no, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let row: GoldenLine = serde_json::from_str(trimmed).expect("parse golden line");
        let ctx = line_ctx(&path, line_no, &row.id);
        assert_golden_line(&ctx, &row);
    }
}
