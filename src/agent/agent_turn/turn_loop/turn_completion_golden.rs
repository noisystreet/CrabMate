//! 完成判定金样回归：`fixtures/turn_completion_golden.jsonl`。

use super::{TurnCompletionDecision, evaluate_turn_redundant_tools};
use crate::cm_names as tool_names;
use crate::test_fixtures::{for_each_jsonl_record, line_ctx};
use crate::types::{FunctionCall, Message, ToolCall};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GoldenLine {
    id: String,
    case: String,
    /// 会话历史 fixture；**只有 `redundant_tools` 档需要**（见 [`fixture_messages`]）。
    #[serde(default)]
    fixture: Option<String>,
    #[serde(default)]
    tool_fixture: Option<String>,
    /// 本回合是否已确认完成（= `PerCoordinator` 缓存的 judge 结论）。
    ///
    /// #1415 之后第 ① 层已删，「是否完成」不再由同步纯逻辑推导，只能由这一位驱动；
    /// 金样因此覆盖 `true` / `false` 两侧。
    #[serde(default)]
    confirmed: bool,
    expect: String,
    #[serde(default)]
    deny_reason: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    via: Option<String>,
}

use crate::test_fixtures::{msg, tool_env};

/// 会话历史 fixture。
///
/// **只有 `redundant_tools` 档用得上**（识别「是否重复了已成功的 run_command」）。
fn fixture_messages(name: &str) -> Vec<Message> {
    match name {
        "readonly_satisfied" => vec![
            msg("user", "分析当前目录"),
            tool_env(tool_names::LIST_TREE, tool_names::LIST_TREE, "list tree: ."),
            msg(
                "assistant",
                "当前目录包含三个压缩包，分析结果如下，总结完成。",
            ),
        ],
        other => panic!("unknown fixture: {other}"),
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
        "glob_files_probe" => vec![ToolCall {
            id: "call_1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: tool_names::GLOB_FILES.into(),
                arguments: r#"{"pattern":"**/*.rs"}"#.into(),
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
    let decision = match row.case.as_str() {
        "redundant_tools" => {
            let fixture = row
                .fixture
                .as_deref()
                .unwrap_or_else(|| panic!("{ctx}: redundant_tools 需要 fixture"));
            let messages = fixture_messages(fixture);
            let tools =
                fixture_tool_calls(row.tool_fixture.as_deref().unwrap_or("list_tree_probe"));
            evaluate_turn_redundant_tools(&tools, &messages, row.confirmed)
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
