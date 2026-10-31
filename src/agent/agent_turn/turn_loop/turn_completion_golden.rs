//! 完成判定金样回归：`fixtures/turn_completion_golden.jsonl`。

use super::{
    TurnCompletionDecision, evaluate_turn_early_stop, evaluate_turn_redundant_tools,
    evaluate_turn_suppress_replanning,
};
use crate::agent::plan_artifact::PlanStepV1;
use crate::test_fixtures::{for_each_jsonl_record, line_ctx};
use crate::types::{FunctionCall, Message, ToolCall};
use serde::Deserialize;
use crate::cm_names as tool_names;

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
    #[allow(dead_code)]
    via: Option<String>,
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
    let parsed = crate::tool_result::parse_tool_output(name, output);
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
            tool_env(tool_names::LIST_TREE, "list tree: ."),
            msg(
                "assistant",
                "当前目录包含三个压缩包，分析结果如下，总结完成。",
            ),
        ],
        "build_satisfied" => vec![
            msg("user", "编译 hpcg"),
            tool_env(
                tool_names::RUN_COMMAND,
                "命令：make\n退出码：0\n标准输出：\nBuilt target hpcg",
            ),
            msg("assistant", "HPCG 编译完成成功。"),
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
        "list_tree_probe" => vec![ToolCall {
            id: "call_1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: tool_names::LIST_TREE.into(),
                arguments: r#"{"path":"."}"#.into(),
            },
        }],
        other => panic!("unknown tool_fixture: {other}"),
    }
}

fn assert_expect_allow(ctx: &str, decision: TurnCompletionDecision) {
    assert!(
        decision.is_allow(),
        "{ctx}: expected allow, got {decision:?}"
    );
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
            let tools = fixture_tool_calls(row.tool_fixture.as_deref().unwrap_or("list_tree_probe"));
            evaluate_turn_redundant_tools(&tools, &messages)
        }
        other => panic!("{ctx}: unknown case {other}"),
    };
    match row.expect.as_str() {
        "allow" => assert_expect_allow(ctx, decision),
        "deny" => assert_expect_deny(ctx, decision, row.deny_reason.as_deref()),
        other => panic!("{ctx}: unknown expect {other}"),
    }
}

#[test]
fn golden_turn_completion_lines_match_decisions() {
    for_each_jsonl_record::<GoldenLine, _>(
        "fixtures/turn_completion_golden.jsonl",
        |path, line_no, row| {
            let ctx = line_ctx(path, line_no, &row.id);
            assert_golden_line(&ctx, &row);
        },
    );
}
