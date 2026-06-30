//! 目标已有完成证据后，抑制冗余探针类 **tool_calls** 与分阶段 **plan steps** 的共享判定。

use crate::agent::plan_artifact::{PlanStepAcceptance, PlanStepV1};
use crate::types::{Message, ToolCall};

use super::run_command_dedupe::{
    normalize_run_command_key, successful_run_command_keys_from_messages,
};

const READONLY_PROBE_TOOL_NAMES: &[&str] = &[
    "read_file",
    "read_dir",
    "list_dir",
    "list_tree",
    "glob",
    "search",
    "extract_in_file",
];

const RUN_COMMAND_VERIFY_MARKERS: &[&str] = &[
    "ls ", "ls -", "stat ", "test -", "file ", "--help", "timeout ", "strace ", " 2>&1",
];

const FOLLOWUP_WRITE_OR_FIX_MARKERS: &[&str] = &[
    "implement",
    "implementation",
    "patch",
    "write",
    "modify",
    "edit",
    "change",
    "fix",
    "repair",
    "refactor",
    "create",
    "add",
    "delete",
    "remove",
    "实现",
    "编写",
    "修改",
    "修复",
    "新增",
    "创建",
    "删除",
    "重构",
    "调整",
];

const REDUNDANT_PROBE_TEXT_MARKERS: &[&str] = &[
    "verify",
    "verification",
    "validate",
    "validation",
    "check",
    "confirm",
    "ensure",
    "exist",
    "exists",
    "rerun",
    "re-run",
    "run again",
    "list",
    "inspect",
    "read",
    "review",
    "summarize",
    "summary",
    "final",
    "report",
    "test",
    "验收",
    "验证",
    "校验",
    "确认",
    "检查",
    "确保",
    "存在",
    "重跑",
    "重新运行",
    "再运行",
    "列出",
    "查看",
    "读取",
    "复查",
    "总结",
    "汇报",
    "最终",
];

fn text_contains_any_marker(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| text.contains(marker))
}

pub(crate) fn tool_calls_are_redundant_after_completion(tool_calls: &[ToolCall]) -> bool {
    tool_calls
        .iter()
        .all(tool_call_is_redundant_after_completion)
}

/// 活跃目标已有完成证据时：探针类 + **已成功过的相同** `run_command` 签名视为冗余。
pub(crate) fn tool_calls_are_redundant_when_goal_satisfied(
    tool_calls: &[ToolCall],
    messages: &[Message],
) -> bool {
    if tool_calls_are_redundant_after_completion(tool_calls) {
        return true;
    }
    let prior_success = successful_run_command_keys_from_messages(messages);
    tool_calls
        .iter()
        .all(|tc| tool_call_is_redundant_build_run_repeat(tc, &prior_success))
}

fn tool_call_is_redundant_build_run_repeat(
    tc: &ToolCall,
    prior_success: &std::collections::HashSet<String>,
) -> bool {
    if tc.function.name != "run_command" {
        return false;
    }
    normalize_run_command_key(tc.function.arguments.as_str())
        .is_some_and(|key| prior_success.contains(&key))
}

pub(crate) fn tool_call_is_redundant_after_completion(tc: &ToolCall) -> bool {
    let name = tc.function.name.as_str();
    if READONLY_PROBE_TOOL_NAMES.contains(&name) {
        return true;
    }
    name == "run_command" && run_command_is_redundant_verification(&tc.function.arguments)
}

fn run_command_is_redundant_verification(args_json: &str) -> bool {
    let Some(invocation) = run_command_invocation_text(args_json) else {
        return false;
    };
    let lower = invocation.to_lowercase();
    text_contains_any_marker(&lower, RUN_COMMAND_VERIFY_MARKERS)
}

fn run_command_invocation_text(args_json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim();
    let mut parts = vec![command.to_string()];
    if let Some(args) = v.get("args").and_then(|x| x.as_array()) {
        parts.extend(args.iter().filter_map(|x| x.as_str()).map(str::to_string));
    }
    Some(parts.join(" "))
}

pub(crate) fn plan_steps_are_redundant_after_completion(steps: &[PlanStepV1]) -> bool {
    steps.iter().all(plan_step_is_redundant_after_completion)
}

pub(crate) fn plan_step_is_redundant_after_completion(step: &PlanStepV1) -> bool {
    let text = redundant_plan_step_text(step);
    if text_contains_any_marker(&text, FOLLOWUP_WRITE_OR_FIX_MARKERS) {
        return false;
    }
    step.acceptance
        .as_ref()
        .is_some_and(PlanStepAcceptance::is_effective)
        || text_contains_any_marker(&text, REDUNDANT_PROBE_TEXT_MARKERS)
}

fn redundant_plan_step_text(step: &PlanStepV1) -> String {
    format!(
        "{}\n{}\n{}",
        step.id,
        step.step_kind.as_deref().unwrap_or_default(),
        step.description
    )
    .to_lowercase()
}

pub(crate) fn redundant_tool_names_for_log(tool_calls: &[ToolCall]) -> Vec<&str> {
    tool_calls
        .iter()
        .map(|tc| tc.function.name.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::PlanStepV1;
    use crate::types::{FunctionCall, ToolCall};

    fn step(id: &str, kind: Option<&str>, description: &str) -> PlanStepV1 {
        PlanStepV1 {
            id: id.to_string(),
            description: description.to_string(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: kind.map(str::to_string),
            acceptance: None,
            max_step_retries: None,
            transitions: None,
        }
    }

    fn run_command_tool(args: &str) -> ToolCall {
        ToolCall {
            id: "tc1".to_string(),
            typ: "function".to_string(),
            function: FunctionCall {
                name: "run_command".to_string(),
                arguments: args.to_string(),
            },
        }
    }

    #[test]
    fn readonly_tools_are_redundant_probes() {
        assert!(tool_call_is_redundant_after_completion(&ToolCall {
            id: "tc1".into(),
            typ: "function".into(),
            function: FunctionCall {
                name: "list_tree".into(),
                arguments: "{}".into(),
            },
        }));
    }

    #[test]
    fn run_command_ls_is_redundant_verification() {
        assert!(tool_call_is_redundant_after_completion(&run_command_tool(
            r#"{"command":"ls","args":["-la"]}"#
        )));
    }

    #[test]
    fn plan_and_tool_share_redundant_probe_semantics() {
        let steps = vec![step("verify-build", Some("verify"), "检查编译产物是否存在")];
        assert!(plan_steps_are_redundant_after_completion(&steps));
        assert!(text_contains_any_marker(
            "检查编译产物是否存在",
            REDUNDANT_PROBE_TEXT_MARKERS
        ));
    }

    #[test]
    fn followup_fix_plan_is_not_redundant() {
        let steps = vec![step(
            "fix-tests",
            Some("implement"),
            "修复失败测试并修改实现",
        )];
        assert!(!plan_steps_are_redundant_after_completion(&steps));
    }

    #[test]
    fn satisfied_goal_marks_exact_run_command_repeat_redundant() {
        use crate::types::Message;
        let messages = vec![Message {
            role: "tool".into(),
            content: Some(
                "$ cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello".into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("tc1".into()),
        }];
        let calls = vec![run_command_tool(
            r#"{"command":"cmake","args":["--build","build"]}"#,
        )];
        assert!(tool_calls_are_redundant_when_goal_satisfied(
            &calls, &messages
        ));
    }

    #[test]
    fn satisfied_goal_does_not_mark_different_run_command_redundant() {
        use crate::types::Message;
        let messages = vec![Message {
            role: "tool".into(),
            content: Some(
                "$ cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello".into(),
            ),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: Some("tc1".into()),
        }];
        let calls = vec![run_command_tool(r#"{"command":"./build/hello","args":[]}"#)];
        assert!(!tool_calls_are_redundant_when_goal_satisfied(
            &calls, &messages
        ));
    }

    #[test]
    fn unsatisfied_gate_still_allows_first_build_command() {
        let calls = vec![run_command_tool(
            r#"{"command":"make","args":["arch=Linux_Serial","-C","hpcg"]}"#,
        )];
        assert!(!tool_calls_are_redundant_after_completion(&calls));
    }
}
