//! 分阶段步内「空执行 / 无构建进展」检测：执行子循环返回 `Ok` 但本分步未产生与验收匹配的实质工具结果。

use crate::agent::acceptance::AcceptanceSpec;
use crate::agent::plan_artifact::{
    PlanStepAcceptance, PlanStepExecutorKind, PlanStepV1,
    plan_step_acceptance_implies_build_progress, plan_step_description_implies_build_execution,
};
use crate::types::{Message, message_content_as_str, staged_step_window_end_exclusive};

/// 步级验收失败原因前缀；补丁规划据此选用更硬的反馈文案。
pub(crate) const STAGED_STEP_EMPTY_EXECUTION_PREFIX: &str = "staged_step_empty_execution:";

/// 自 `step_user_index` 指向的分步 `user` 起，至下一条 `user` 或末尾，是否出现过 `role: tool`。
pub(crate) fn staged_step_window_has_tool(messages: &[Message], step_user_index: usize) -> bool {
    staged_step_window_tool_entries(messages, step_user_index)
        .next()
        .is_some()
}

fn staged_step_window_tool_entries<'a>(
    messages: &'a [Message],
    step_user_index: usize,
) -> impl Iterator<Item = (&'a str, &'a str)> + 'a {
    let end = staged_step_window_end_exclusive(messages, step_user_index);
    let mut i = step_user_index.saturating_add(1);
    std::iter::from_fn(move || {
        while i < end {
            let m = &messages[i];
            if m.role == "tool" {
                let name = m.name.as_deref().unwrap_or("");
                let content = message_content_as_str(&m.content).unwrap_or("");
                i += 1;
                return Some((name, content));
            }
            i += 1;
        }
        None
    })
}

pub(crate) fn staged_step_window_has_build_progress_tool(
    messages: &[Message],
    step_user_index: usize,
) -> bool {
    staged_step_window_tool_entries(messages, step_user_index)
        .any(|(name, content)| tool_message_indicates_build_progress(name, content))
}

pub(crate) fn tool_message_indicates_build_progress(tool_name: &str, content: &str) -> bool {
    if matches!(
        tool_name,
        "run_executable"
            | "cargo_test"
            | "cargo_check"
            | "cargo_clippy"
            | "cargo_fmt_check"
            | "pytest_run"
            | "go_test"
            | "cppcheck_analyze"
            | "shellcheck_check"
    ) {
        return true;
    }
    if tool_name != "run_command" {
        return false;
    }
    let lower = content.to_lowercase();
    const BUILD_MARKERS: &[&str] = &[
        "make ",
        "make\n",
        "cmake --build",
        "cmake -build",
        "cargo build",
        "cargo test",
        "cargo run",
        "ninja ",
        "meson compile",
        "meson test",
        "go build",
        "npm run build",
        "npm test",
        "ctest ",
        "bazel build",
        "buck build",
    ];
    BUILD_MARKERS.iter().any(|marker| lower.contains(marker))
}

fn plan_step_expects_build_progress(step: &PlanStepV1) -> bool {
    if step.executor_kind == Some(PlanStepExecutorKind::TestRunner) {
        return true;
    }
    if step
        .acceptance
        .as_ref()
        .filter(|a| PlanStepAcceptance::is_effective(a))
        .is_some_and(plan_step_acceptance_implies_build_progress)
    {
        return true;
    }
    plan_step_description_implies_build_execution(step.description.as_str())
}

pub(crate) fn staged_step_empty_execution_reason() -> String {
    format!(
        "{STAGED_STEP_EMPTY_EXECUTION_PREFIX} 本步执行子循环已结束，但本分步内未产生任何 `role: tool` 工具结果；\
         不得仅用自然语言描述「将要读取/编译/执行」，须通过 API 结构化 `tool_calls` 实际调用工具。"
    )
}

fn staged_step_no_build_progress_reason() -> String {
    format!(
        "{STAGED_STEP_EMPTY_EXECUTION_PREFIX} 本步仅有只读/探针类工具结果（如列目录、读文件、`--version`），\
         未执行构建/测试类命令，但当前步验收或角色要求构建/运行进展。"
    )
}

pub(crate) fn staged_step_empty_execution_is_reason(reason: &str) -> bool {
    reason.starts_with(STAGED_STEP_EMPTY_EXECUTION_PREFIX)
}

/// 在外层 `run_agent_outer_loop` 返回 `Ok` 后判定本步是否因「零工具 / 无构建进展」应记为验收失败。
pub(crate) fn staged_step_empty_execution_verify_failure(
    messages: &[Message],
    step_user_index: usize,
    step: &PlanStepV1,
    workspace_root: &std::path::Path,
) -> Option<String> {
    let has_tools = staged_step_window_has_tool(messages, step_user_index);
    if !has_tools {
        if let Some(acceptance) = step
            .acceptance
            .as_ref()
            .filter(|a| PlanStepAcceptance::is_effective(a))
        {
            let spec = AcceptanceSpec::from(acceptance);
            if !spec.requires_tool_evidence()
                && crate::agent::step_verifier::verify_step_execution(
                    acceptance,
                    messages,
                    step_user_index,
                    workspace_root,
                )
                .is_pass()
            {
                return None;
            }
        }
        return Some(staged_step_empty_execution_reason());
    }

    if plan_step_expects_build_progress(step)
        && !staged_step_window_has_build_progress_tool(messages, step_user_index)
    {
        return Some(staged_step_no_build_progress_reason());
    }

    None
}

pub(crate) fn staged_step_empty_execution_patch_detail(
    reason: &str,
    acceptance_ref: Option<&PlanStepAcceptance>,
) -> String {
    let reference_line = acceptance_ref
        .and_then(|a| a.compact_reference_for_planner_feedback())
        .map(|line| format!("- **参考验收（acceptance，r）**：{line}\n"))
        .unwrap_or_default();
    format!(
        "### 偏差结构化（空执行 / 无构建进展）\n\
         {reference_line}\
         - **观测**：{reason}\n\
         **硬约束**：下一版规划中，本步执行器**必须**至少产生一条成功的工具调用（如 `read_file` / `read_dir` / `run_command` / `archive_unpack` 等），\
         禁止再输出仅承诺「将要查看文档/目录/构建说明」而无 `tool_calls` 的助手正文。\n\
         若用户目标是编译/构建/测试：须在本步或后续 `test_runner` 步实际执行构建/测试命令（`run_command` 等），\
         且 `acceptance` 只检查**该步实际能产出的**证据（只读/解压步勿验收可执行二进制或退出码）。\n\
         请缩短后续步骤并修复本步。"
    )
}

pub(crate) fn staged_step_patch_exhausted_build_hint(
    messages: &[Message],
    step_user_index: usize,
    user_goal: Option<&str>,
) -> Option<String> {
    let goal_implies_build = user_goal.is_some_and(|g| {
        plan_step_description_implies_build_execution(g) || g.to_lowercase().contains("编译")
    });
    if !goal_implies_build {
        return None;
    }
    if staged_step_window_has_build_progress_tool(messages, step_user_index) {
        return None;
    }
    Some(
        "提示：本分步多次补丁重试后仍未出现构建/测试类命令（如 `make`、`cmake --build`、`cargo build` 等）；\
         请检查规划是否将 `acceptance` 或 `test_runner` 错放在只读/解压步，或执行器是否未调用 `run_command`。"
            .to_string(),
    )
}

pub(crate) fn staged_step_retry_exhausted_message_body(
    base: String,
    messages: &[Message],
    step_user_index: usize,
    user_goal: Option<&str>,
    audit_footer: &str,
) -> String {
    let mut s = base;
    if let Some(hint) = staged_step_patch_exhausted_build_hint(messages, step_user_index, user_goal)
    {
        s.push_str("\n\n");
        s.push_str(&hint);
    }
    s.push_str(audit_footer);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::plan_acceptance_path_looks_like_build_artifact;
    use crate::types::Message;

    fn user(text: &str) -> Message {
        Message::user_only(text)
    }

    fn asst(text: &str) -> Message {
        Message::assistant_only(text)
    }

    fn tool(name: &str, body: &str) -> Message {
        Message {
            role: "tool".into(),
            content: Some(body.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: Some(name.into()),
            tool_call_id: Some("tc1".into()),
        }
    }

    fn step_with_binary_acceptance() -> PlanStepV1 {
        PlanStepV1 {
            id: "s1".into(),
            description: "unpack sources".into(),
            workflow_node_id: None,
            executor_kind: None,
            step_kind: None,
            acceptance: Some(PlanStepAcceptance {
                expect_exit_code: None,
                expect_stdout_contains: None,
                expect_stderr_contains: None,
                expect_file_exists: Some("proj/bin/app".into()),
                expect_json_path_equals: None,
                expect_http_status: None,
            }),
            max_step_retries: None,
            transitions: None,
        }
    }

    #[test]
    fn window_detects_tool_after_step_user() {
        let msgs = vec![
            user("goal"),
            user("step 1"),
            asst("working"),
            tool("list_dir", "ok"),
        ];
        assert!(staged_step_window_has_tool(&msgs, 1));
    }

    #[test]
    fn window_spans_tools_after_orchestration_injection() {
        let msgs = vec![
            user("编译项目"),
            Message::user_staged_step_injection("### 分步 1/1\n- id: s1\n- 描述: build\n"),
            Message::user_staged_orchestration_injection("【编排纠偏】请继续构建"),
            tool("run_command", "make ok"),
        ];
        assert!(staged_step_window_has_tool(&msgs, 1));
    }

    #[test]
    fn window_false_when_only_assistant_prose() {
        let msgs = vec![
            user("goal"),
            user("step 1"),
            asst("will read the build docs"),
        ];
        assert!(!staged_step_window_has_tool(&msgs, 1));
    }

    #[test]
    fn readonly_tools_are_not_build_progress() {
        let msgs = vec![
            user("goal"),
            user("step 1"),
            tool("read_file", "ok"),
            tool("run_command", "命令：gcc --version\n退出码：0"),
        ];
        assert!(!staged_step_window_has_build_progress_tool(&msgs, 1));
    }

    #[test]
    fn make_command_counts_as_build_progress() {
        let msgs = vec![
            user("goal"),
            user("step 1"),
            tool(
                "run_command",
                "命令：make arch=Linux_Serial\n退出码：0\n标准输出：",
            ),
        ];
        assert!(staged_step_window_has_build_progress_tool(&msgs, 1));
    }

    #[test]
    fn no_build_progress_when_only_probes_and_binary_acceptance() {
        let msgs = vec![
            user("goal"),
            user("step 1"),
            tool("archive_unpack", "ok"),
            tool("run_command", "命令：gcc --version\n退出码：0"),
        ];
        let step = step_with_binary_acceptance();
        let fail =
            staged_step_empty_execution_verify_failure(&msgs, 1, &step, std::path::Path::new("."));
        assert!(fail.is_some());
        assert!(staged_step_empty_execution_is_reason(
            fail.as_deref().unwrap()
        ));
    }

    #[test]
    fn empty_execution_reason_prefix_stable() {
        let r = staged_step_empty_execution_reason();
        assert!(staged_step_empty_execution_is_reason(&r));
    }

    #[test]
    fn patch_exhausted_hint_when_build_goal_and_no_make() {
        let msgs = vec![user("编译项目"), user("step"), tool("list_dir", "ok")];
        let hint = staged_step_patch_exhausted_build_hint(&msgs, 1, Some("编译项目"));
        assert!(hint.is_some());
    }

    #[test]
    fn plan_acceptance_path_helper_public() {
        assert!(plan_acceptance_path_looks_like_build_artifact(
            "proj/bin/app"
        ));
    }
}
