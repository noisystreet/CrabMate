//! 目标已有完成证据后，抑制冗余探针类 **tool_calls** 与分阶段 **plan steps** 的共享判定。

use crate::cm_agent::plan_artifact::{PlanStepAcceptance, PlanStepExecutorKind, PlanStepV1};
use crate::cm_names as tool_names;
use crate::cm_types::{Message, ToolCall};

use super::run_command_dedupe::{
    normalize_run_command_key, successful_run_command_keys_from_messages,
};

/// 只读「探测」类工具：目标已有完成证据时，再调一次不会带回新信息。
///
/// `read_file` / `read_dir` / `list_tree` 是列举与读取，`glob_files` / `search_in_files`
/// 是查找——同属一族，抑制口径必须一致。历史上这里写的是 `list_dir` / `glob` / `search`
/// （三个并不存在的工具名，见 #1155 / #1204），导致查找类两个真实工具从未生效。
const READONLY_PROBE_TOOL_NAMES: &[&str] = &[
    tool_names::READ_FILE,
    tool_names::READ_DIR,
    tool_names::LIST_TREE,
    tool_names::GLOB_FILES,
    tool_names::SEARCH_IN_FILES,
];

/// 只读探测程序：`basename(argv[0])` **精确**匹配（#1422 A1）。
///
/// 历史实现把这些词拼进 `command + args` 串再 `contains`，于是 `cargo test -p x` 命中
/// `test -`、`grep Makefile -n` 命中 `file `、`bash -c "… 2>&1"` 命中 ` 2>&1`、
/// `timeout 300 make` 命中 `timeout `——把真实验证与真实构建判成「只读探测」而抑制掉。
/// 命令是结构化字段，按 argv 判定，不看拼串。
const READONLY_PROBE_PROGRAMS: &[&str] = &["ls", "stat", "file", "test", "readlink", "realpath"];

/// 只问用法/版本，不产生副作用——任何程序带这些 flag 都算探测
/// （`cargo build --help` 确实不构建）。
const PROBE_USAGE_FLAGS: &[&str] = &["--help", "--version"];

/// 包装器：自身不改变「内层命令是否只读」，必须剥壳递归判定。
/// `timeout 300 make` 是构建，`timeout 5 ls` 才是探测。
const PROBE_WRAPPER_PROGRAMS: &[&str] = &[
    "timeout", "strace", "env", "nice", "nohup", "command", "exec", "sudo",
];

/// `bash -c` 等：真实命令在 `-c` 的脚本文本里。
const SHELL_WRAPPER_FLAGS: &[&str] = &["-c", "-lc", "-ic", "-ec"];

/// 剥壳深度上限，防 `timeout 5 timeout 5 …` 无限递归。
const MAX_PROBE_WRAP_DEPTH: usize = 3;

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

pub fn tool_calls_are_redundant_after_completion(tool_calls: &[ToolCall]) -> bool {
    tool_calls
        .iter()
        .all(tool_call_is_redundant_after_completion)
}

/// 活跃目标已有完成证据时：探针类 + **已成功过的相同** `run_command` 签名视为冗余。
pub fn tool_calls_are_redundant_when_goal_satisfied(
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
    if tc.function.name != tool_names::RUN_COMMAND {
        return false;
    }
    normalize_run_command_key(tc.function.arguments.as_str())
        .is_some_and(|key| prior_success.contains(&key))
}

pub fn tool_call_is_redundant_after_completion(tc: &ToolCall) -> bool {
    let name = tc.function.name.as_str();
    if READONLY_PROBE_TOOL_NAMES.contains(&name) {
        return true;
    }
    name == tool_names::RUN_COMMAND && run_command_is_redundant_verification(&tc.function.arguments)
}

fn run_command_is_redundant_verification(args_json: &str) -> bool {
    let Some((command, args)) = run_command_argv(args_json) else {
        return false;
    };
    argv_is_readonly_probe(&command, &args, 0)
}

/// 取出 `command` 与 `args` 两个**结构字段**（#1422 A1：不再拼成一个串）。
fn run_command_argv(args_json: &str) -> Option<(String, Vec<String>)> {
    let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
    let command = v.get("command")?.as_str()?.trim().to_lowercase();
    if command.is_empty() {
        return None;
    }
    let args = v
        .get("args")
        .and_then(|x| x.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .map(str::to_lowercase)
                .collect()
        })
        .unwrap_or_default();
    Some((command, args))
}

/// 该次调用是否**只是**只读探测：目标已有完成证据时，再跑一次不会带回新信息。
fn argv_is_readonly_probe(command: &str, args: &[String], depth: usize) -> bool {
    if depth > MAX_PROBE_WRAP_DEPTH {
        return false;
    }
    if args
        .iter()
        .any(|a| PROBE_USAGE_FLAGS.contains(&a.as_str()))
    {
        return true;
    }
    let base = program_basename(command);
    if is_probe_shell_wrapper(base) {
        return shell_script_argv(args)
            .is_some_and(|(cmd, rest)| argv_is_readonly_probe(&cmd, &rest, depth.saturating_add(1)));
    }
    if PROBE_WRAPPER_PROGRAMS.contains(&base) {
        return unwrap_wrapper_argv(base, args).is_some_and(|(cmd, rest)| {
            argv_is_readonly_probe(&cmd, &rest, depth.saturating_add(1))
        });
    }
    READONLY_PROBE_PROGRAMS.contains(&base)
}

/// `basename`：`/bin/ls` 与 `ls` 同义。
fn program_basename(command: &str) -> &str {
    command.rsplit('/').next().unwrap_or_default()
}

fn is_probe_shell_wrapper(prog: &str) -> bool {
    matches!(prog, "bash" | "sh" | "zsh" | "dash")
}

/// `bash -c "ls -la"`：把 `-c` 之后拼回脚本再切成 argv。
///
/// 引号边界在这一步会丢，但判定只看 `argv[0]` 与 flag，够用；故意**不**做完整的
/// shell 解析——那会把「只读探测」判定变成一个小 shell。
fn shell_script_argv(args: &[String]) -> Option<(String, Vec<String>)> {
    let pos = args
        .iter()
        .position(|t| SHELL_WRAPPER_FLAGS.contains(&t.as_str()))?;
    let script = args[pos.saturating_add(1)..].join(" ");
    let mut tokens = script.split_whitespace().map(str::to_string);
    let cmd = tokens.next()?;
    Some((cmd, tokens.collect()))
}

/// 剥掉包装器自身的 flag 与 `VAR=val`，返回（内层命令, 内层 args）。
fn unwrap_wrapper_argv(prog: &str, args: &[String]) -> Option<(String, Vec<String>)> {
    let mut i = 0usize;
    if prog == "timeout" {
        // `timeout` 的首个非 flag token 是时长（`60` / `5s`），必须跳过，
        // 否则 `timeout 60 make` 会把 `60` 当成内层命令名。
        while let Some(t) = args.get(i) {
            i = i.saturating_add(1);
            if !t.starts_with('-') {
                break;
            }
        }
    }
    while let Some(t) = args.get(i) {
        if t.starts_with('-') || is_env_assignment(t) {
            i = i.saturating_add(1);
        } else {
            break;
        }
    }
    let cmd = args.get(i)?.clone();
    Some((cmd, args[i.saturating_add(1)..].to_vec()))
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty()
        && !name.starts_with('-')
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == 95)
}

pub fn plan_steps_are_redundant_after_completion(steps: &[PlanStepV1]) -> bool {
    steps.iter().all(plan_step_is_redundant_after_completion)
}

/// 须由分阶段步执行器实际跑工具/验收，不可被任务级早停或重复规划抑制跳过。
pub(crate) fn plan_step_requires_formal_execution(step: &PlanStepV1) -> bool {
    step.acceptance
        .as_ref()
        .is_some_and(PlanStepAcceptance::is_effective)
        || matches!(step.executor_kind, Some(PlanStepExecutorKind::TestRunner))
}

pub fn plan_steps_require_formal_execution(steps: &[PlanStepV1]) -> bool {
    steps.iter().any(plan_step_requires_formal_execution)
}

pub(crate) fn plan_step_is_redundant_after_completion(step: &PlanStepV1) -> bool {
    if plan_step_requires_formal_execution(step) {
        return false;
    }
    let text = redundant_plan_step_text(step);
    if text_contains_any_marker(&text, FOLLOWUP_WRITE_OR_FIX_MARKERS) {
        return false;
    }
    text_contains_any_marker(&text, REDUNDANT_PROBE_TEXT_MARKERS)
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

pub fn redundant_tool_names_for_log(tool_calls: &[ToolCall]) -> Vec<&str> {
    tool_calls
        .iter()
        .map(|tc| tc.function.name.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::{FunctionCall, ToolCall};

    use crate::test_fixtures::step;

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

    /// #1422 A1 误命中 golden：这些命令拼串后都会命中旧表 marker，被误当只读探测。
    #[test]
    fn real_verify_and_build_commands_are_not_readonly_probes() {
        for args in [
            r#"{"command":"cargo","args":["test","-p","crabmate"]}"#,
            r#"{"command":"cargo","args":["build","--release"]}"#,
            r#"{"command":"bash","args":["-c","cmake --build build 2>&1 | tail -50"]}"#,
            r#"{"command":"grep","args":["Makefile","-n"]}"#,
            r#"{"command":"timeout","args":["300","make","-j8"]}"#,
            r#"{"command":"strace","args":["-f","make"]}"#,
            r#"{"command":"./build/hello","args":[]}"#,
        ] {
            assert!(
                !run_command_is_redundant_verification(args),
                "真实验证/构建不应判成只读探测：{args}"
            );
        }
    }

    #[test]
    fn readonly_probes_are_still_redundant() {
        for args in [
            r#"{"command":"ls","args":["-la"]}"#,
            r#"{"command":"/bin/ls","args":[]}"#,
            r#"{"command":"stat","args":["target"]}"#,
            r#"{"command":"file","args":["a.out"]}"#,
            r#"{"command":"test","args":["-f","a.out"]}"#,
            r#"{"command":"cargo","args":["build","--help"]}"#,
            r#"{"command":"bash","args":["-c","ls -la"]}"#,
            r#"{"command":"timeout","args":["5","ls","-la"]}"#,
            r#"{"command":"env","args":["A=1","ls"]}"#,
        ] {
            assert!(
                run_command_is_redundant_verification(args),
                "只读探测应判冗余：{args}"
            );
        }
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
    fn plan_step_with_effective_acceptance_is_not_redundant() {
        let steps = vec![PlanStepV1 {
            id: "run-tests".into(),
            description: "运行 cargo test 验收".into(),
            workflow_node_id: None,
            executor_kind: Some(crate::cm_agent::plan_artifact::PlanStepExecutorKind::TestRunner),
            step_kind: None,
            acceptance: Some(PlanStepAcceptance {
                expect_exit_code: Some(0),
                ..Default::default()
            }),
            max_step_retries: None,
            transitions: None,
        }];
        assert!(!plan_steps_are_redundant_after_completion(&steps));
    }

    #[test]
    fn satisfied_goal_marks_exact_run_command_repeat_redundant() {
        use crate::cm_types::Message;
        let messages = vec![Message::tool_result("tc1", "$ cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello")];
        let calls = vec![run_command_tool(
            r#"{"command":"cmake","args":["--build","build"]}"#,
        )];
        assert!(tool_calls_are_redundant_when_goal_satisfied(
            &calls, &messages
        ));
    }

    #[test]
    fn satisfied_goal_does_not_mark_different_run_command_redundant() {
        use crate::cm_types::Message;
        let messages = vec![Message::tool_result("tc1", "$ cmake --build build\n退出码：0\n标准输出：\n[100%] Built target hello")];
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
