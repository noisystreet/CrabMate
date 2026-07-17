//! 分阶段步内「空执行 / 无构建进展」检测：执行子循环返回 `Ok` 但本分步未产生与验收匹配的实质工具结果。
//!
//! 生产路径已不再调用（步验收改走 step_verifier），函数保留供测试模块引用。

use crate::agent::plan_artifact::PlanStepAcceptance;
use crate::types::{Message, message_content_as_str, tool_messages_in_staged_step_window};

/// 步级验收失败原因前缀；补丁规划据此选用更硬的反馈文案。
pub(crate) const STAGED_STEP_EMPTY_EXECUTION_PREFIX: &str = "staged_step_empty_execution:";

/// 自 `step_user_index` 指向的分步 `user` 起，至下一条 `user` 或末尾，是否出现过 `role: tool`。
pub(crate) fn staged_step_window_has_tool(messages: &[Message], step_user_index: usize) -> bool {
    !tool_messages_in_staged_step_window(messages, step_user_index).is_empty()
}

pub(crate) fn staged_step_empty_execution_is_reason(reason: &str) -> bool {
    reason.starts_with(STAGED_STEP_EMPTY_EXECUTION_PREFIX)
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

/// 本分步多次补丁重试后仍未出现构建进展时的提示，无构建目标时返回 `None`。
fn staged_step_patch_exhausted_build_hint(
    messages: &[Message],
    step_user_index: usize,
    user_goal: Option<&str>,
) -> Option<String> {
    let goal_implies_build = user_goal.is_some_and(|g| {
        crate::agent::plan_artifact::plan_step_description_implies_build_execution(g)
            || g.to_lowercase().contains("编译")
    });
    if !goal_implies_build {
        return None;
    }
    if tool_messages_in_staged_step_window(messages, step_user_index)
        .iter()
        .any(|m| tool_message_indicates_build_progress(m))
    {
        return None;
    }
    Some(
        "提示：本分步多次补丁重试后仍未出现构建/测试类命令（如 `make`、`cmake --build`、`cargo build` 等）；\
         请检查规划是否将 `acceptance` 或 `test_runner` 错放在只读/解压步，或执行器是否未调用 `run_command`。"
            .to_string(),
    )
}

pub(crate) fn tool_message_indicates_build_progress(m: &Message) -> bool {
    let Some(name) = m.name.as_deref() else {
        return false;
    };
    if matches!(
        name,
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
    if name != "run_command" {
        return false;
    }
    let Some(content) = message_content_as_str(&m.content) else {
        return false;
    };
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
