//! 子目标验证器（GoalVerifier）：验证子目标是否达成
//!
//! 根据 SubGoal 中定义的 acceptance 条件对执行结果进行验证。
//! 与分阶段步骤共用 [`crate::agent::acceptance`] 内核（文件 / 合并输出 / 退出码等）。

use crate::agent::acceptance::{VerifyOutcome, parse_exit_code_from_combined_output};

use super::goal_acceptance::{effective_goal_acceptance, verify_goal_acceptance_spec};
use super::task::{ArtifactKind, SubGoal, TaskResult, TaskStatus};

/// 验证结果
#[derive(Debug, Clone)]
pub enum VerificationResult {
    /// 验证通过
    Pass,
    /// 验证失败
    Fail { reason: String },
    /// 需要人工确认
    EscalateHuman { reason: String },
}

impl VerificationResult {
    pub fn is_pass(&self) -> bool {
        matches!(self, VerificationResult::Pass)
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, VerificationResult::Fail { .. })
    }
}

/// 将共用内核的英文 machine reason 映射回分层历史中文文案（仅覆盖当前会触发的分支）。
fn localize_hierarchy_verify_reason(reason: String) -> String {
    if let Some(suffix) = reason.strip_prefix("exit_code_mismatch: expected ")
        && let Some((exp, got_part)) = suffix.split_once(", got ")
    {
        return format!(
            "退出码不匹配: 期望 {}, 实际 {}",
            exp.trim(),
            got_part.trim()
        );
    }
    reason
}

/// 子目标验证器
pub struct GoalVerifier {
    workspace_root: std::path::PathBuf,
}

impl GoalVerifier {
    /// 创建新的验证器
    pub fn new(workspace_root: std::path::PathBuf) -> Self {
        Self { workspace_root }
    }

    /// 验证子目标是否达成
    ///
    /// 检查顺序：
    /// 1. 执行结果状态
    /// 2. 目标级硬门槛（编写运行程序 / 运行可执行体证据）
    /// 3. [`GoalAcceptance`] 字段：文件 / stdout·stderr / 合并输出 / JSON path / HTTP / 退出码（经 [`crate::agent::acceptance`]）
    /// 4. `expect_command_success` 二次验证命令
    pub fn verify(&self, goal: &SubGoal, result: &TaskResult) -> VerificationResult {
        // 首先检查执行结果状态
        match &result.status {
            TaskStatus::Completed => {
                // 执行完成，继续验证 acceptance 条件
            }
            TaskStatus::Failed { reason } => {
                return VerificationResult::Fail {
                    reason: format!("子目标执行失败: {}", reason),
                };
            }
            TaskStatus::Skipped { reason } => {
                return VerificationResult::Fail {
                    reason: format!("子目标被跳过: {}", reason),
                };
            }
            _ => {
                return VerificationResult::Fail {
                    reason: "子目标未处于完成状态".to_string(),
                };
            }
        }

        // 对“编写并执行程序”类目标启用硬门槛：必须具备写源码 + 编译 + 运行证据，避免只 read_dir 也被判完成。
        if is_program_build_and_run_goal(goal)
            && let Err(reason) = verify_program_build_and_run_evidence(result)
        {
            return VerificationResult::Fail { reason };
        }

        let expected_output_hints: Vec<String> = goal
            .acceptance
            .as_ref()
            .map(|a| {
                a.expect_output_contains
                    .iter()
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        // 「运行可执行体」子目标：须出现 run_executable，或 run_command 且可核对到进程级输出
        if is_run_executable_subgoal(goal)
            && let Err(reason) =
                verify_run_executable_subgoal_tool_evidence(result, &expected_output_hints)
        {
            return VerificationResult::Fail { reason };
        }

        // 有效验收条件（与分阶段 `effective_plan_step_acceptance` 共用缺省启发式）。
        let acceptance = match effective_goal_acceptance(goal) {
            Some(a) => a,
            None => {
                log::info!(
                    target: "crabmate",
                    "[GOAL_VERIFIER] Goal {} has no acceptance criteria, passing by default",
                    goal.goal_id
                );
                return VerificationResult::Pass;
            }
        };

        log::info!(
            target: "crabmate",
            "[GOAL_VERIFIER] Verifying goal {} with acceptance criteria",
            goal.goal_id
        );

        match verify_goal_acceptance_spec(&acceptance, result, self.workspace_root.as_path()) {
            VerifyOutcome::Pass => {}
            VerifyOutcome::Fail { reason } => {
                return VerificationResult::Fail {
                    reason: localize_hierarchy_verify_reason(reason),
                };
            }
        }

        // 验证命令成功执行（如果有定义）
        if let Some(cmd) = &acceptance.expect_command_success {
            match self.run_verify_command(cmd) {
                Ok(true) => {}
                Ok(false) => {
                    return VerificationResult::Fail {
                        reason: format!("验证命令执行失败: {}", cmd),
                    };
                }
                Err(e) => {
                    return VerificationResult::Fail {
                        reason: format!("验证命令出错: {}", e),
                    };
                }
            }
        }

        log::info!(
            target: "crabmate",
            "[GOAL_VERIFIER] Goal {} verification passed",
            goal.goal_id
        );

        VerificationResult::Pass
    }

    /// 运行验证命令
    fn run_verify_command(&self, cmd: &str) -> Result<bool, String> {
        use std::process::Command;

        log::info!(
            target: "crabmate",
            "[GOAL_VERIFIER] Running verify command: {}",
            cmd
        );

        // 解析命令和参数
        let parts: Vec<&str> = cmd.split_whitespace().collect();
        if parts.is_empty() {
            return Err("空命令".to_string());
        }

        let command = parts[0];
        let args = &parts[1..];

        let output = Command::new(command)
            .args(args)
            .current_dir(&self.workspace_root)
            .output()
            .map_err(|e| format!("执行命令失败: {}", e))?;

        let success = output.status.success();
        let exit_code = output.status.code().unwrap_or(-1);

        log::info!(
            target: "crabmate",
            "[GOAL_VERIFIER] Verify command exit code: {}",
            exit_code
        );

        Ok(success)
    }
}

/// 子目标是否要求「真正运行已构建的可执行体」（与仅「检查产物是否生成」区分，仅用于分层模式验收）。
pub(crate) fn is_run_executable_subgoal(goal: &SubGoal) -> bool {
    let d = goal.description.to_lowercase();
    if run_exec_subgoal_excluded_build_only(&d) {
        return false;
    }
    if run_exec_subgoal_excluded_cmake_build_compile(&d) {
        return false;
    }
    if run_exec_subgoal_excluded_check_only(&d) {
        return false;
    }
    run_exec_subgoal_positive_signals(&d)
}

fn run_exec_subgoal_excluded_build_only(d: &str) -> bool {
    (d.contains("编译")
        || d.contains("构建")
        || d.contains("cmake --build")
        || d.contains("linking")
        || d.contains("built target"))
        && !d.contains("运行")
        && !d.contains("执行")
        && !d.contains("run ")
        && !d.contains("run_executable")
        && !d.contains("./")
}

fn run_exec_subgoal_excluded_cmake_build_compile(d: &str) -> bool {
    d.contains("cmake --build")
        && d.contains("编译")
        && d.contains("生成可执行")
        && !d.contains("./")
        && !d.contains("run_executable")
}

fn run_exec_subgoal_excluded_check_only(d: &str) -> bool {
    d.contains("检查")
        && (d.contains("是否生成")
            || d.contains("是否已生成")
            || d.contains("是否存在")
            || d.contains("是否产生"))
        && !d.contains("运行")
}

fn run_exec_subgoal_positive_run_verbs(d: &str) -> bool {
    d.contains("运行")
        || d.contains("并运行")
        || d.contains("跑")
        || d.contains("run the")
        || d.contains("run ")
}

fn run_exec_subgoal_positive_targets(d: &str) -> bool {
    d.contains("可执行")
        || d.contains("executable")
        || d.contains("可执行文件")
        || (d.contains("编译")
            && d.contains("生成")
            && (d.contains("运行") || d.contains("执行") || d.contains("验证")))
}

fn run_exec_subgoal_positive_signals(d: &str) -> bool {
    let has_run_verb = run_exec_subgoal_positive_run_verbs(d);
    let has_target = run_exec_subgoal_positive_targets(d);
    (has_run_verb && (has_target || d.contains("产物")))
        || (d.contains("验证") && d.contains("输出") && (d.contains("运行") || d.contains("程序")))
        || (d.contains("验证") && d.contains("hello"))
}

/// 对「运行可执行体」子目标，必须有 `run_executable` 或充分的 `run_command` 执行产物证据
fn verify_run_executable_subgoal_tool_evidence(
    result: &TaskResult,
    expected_output_hints: &[String],
) -> Result<(), String> {
    if result.tools_invoked.iter().any(|n| n == "run_executable") {
        return Ok(());
    }
    if !result.tools_invoked.iter().any(|n| n == "run_command") {
        return Err(
            "缺少 `run_executable` 或能证明已执行可执行产物的 `run_command`。请用 `run_executable` + 工作区相对路径（如 `build/目标名`）运行产物。"
                .to_string(),
        );
    }
    let c = format!(
        "{}\n{}",
        result.output.as_deref().unwrap_or(""),
        result.error.as_deref().unwrap_or("")
    );
    // 在「同一次 run_command 工具行」的片段内出现 Hello，避免与 read_file 里读到的源码混淆
    if run_command_invocation_matches_expected_output(&c, expected_output_hints) {
        return Ok(());
    }
    Err(format!(
        "子目标为「运行可执行体」：已调用 `run_command` 但未能核对到可执行体进程级输出；请用 `run_executable` 或 `run_command` 直接执行构建产物，并产生可核对输出（如 Hello, World!）。{}",
        run_exec_verification_diag(result)
    ))
}

fn run_exec_verification_diag(result: &TaskResult) -> String {
    let tools = if result.tools_invoked.is_empty() {
        "(none)".to_string()
    } else {
        result.tools_invoked.join(",")
    };
    let out = truncate_verify_preview(result.output.as_deref().unwrap_or(""), 200);
    let err = truncate_verify_preview(result.error.as_deref().unwrap_or(""), 200);
    let exit = parse_exit_code_from_combined_output(&format!("{}\n{}", out, err))
        .map(|c| c.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    format!(
        " [diag tools_invoked={tools} exit_code={exit} output_preview={out:?} error_preview={err:?}]"
    )
}

fn truncate_verify_preview(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

pub(crate) fn run_command_invocation_matches_expected_output(
    c: &str,
    expected_output_hints: &[String],
) -> bool {
    fn matches_expected(t: &str, expected_output_hints: &[String]) -> bool {
        let l = t.to_lowercase();
        let hints: Vec<String> = if expected_output_hints.is_empty() {
            vec!["hello, world!".to_string(), "hello, world".to_string()]
        } else {
            expected_output_hints
                .iter()
                .map(|s| s.trim().to_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        };
        hints.into_iter().any(|h| l.contains(&h))
    }
    /// 单次 `Tool run_command …` 片段内是否像「成功执行且 stdout 含 Hello」
    fn run_command_section_indicates_hello_run(
        seg: &str,
        expected_output_hints: &[String],
    ) -> bool {
        let t = format!("Tool run_command{seg}");
        let l = t.to_lowercase();
        let looks_ok = l.contains("executed successfully")
            || l.contains("退出码：0")
            || l.contains("退出码:0")
            || l.contains("(exit=0)");
        if !looks_ok || !matches_expected(&t, expected_output_hints) {
            return false;
        }
        // 有「标准输出：」+ Hello 时，避免误把 read_file/cat 里的源码行计为运行结果
        if t.contains("标准输出") {
            return !l.contains("#include <iostream>");
        }
        matches_expected(&t, expected_output_hints) && !l.contains("#include <iostream>")
    }
    if c.split("Tool run_command")
        .skip(1)
        .any(|seg| run_command_section_indicates_hello_run(seg, expected_output_hints))
    {
        return true;
    }
    // 子目标 trace / 去英文前缀等：须同时出现「可核对成功」与 run_command 语境，防把其它工具输出串进来误判
    let low = c.to_lowercase();
    if low.contains("标准输出")
        && (low.contains("退出码：0") || low.contains("退出码:0") || low.contains("(exit=0)"))
        && matches_expected(c, expected_output_hints)
        && low.contains("run_command")
        && (low.contains("subgoal_tool_trace")
            || low.contains("executed successfully")
            || low.contains("tool run_command"))
        && !low.contains("#include <iostream>")
    {
        return true;
    }
    low.contains("subgoal_tool_trace")
        && low.contains("run_command")
        && matches_expected(c, expected_output_hints)
        && !low.contains("#include <iostream>")
}

pub(crate) fn run_command_invocation_mentions_hello(c: &str) -> bool {
    run_command_invocation_matches_expected_output(c, &[])
}

fn is_program_build_and_run_goal(goal: &SubGoal) -> bool {
    let d = goal.description.to_lowercase();
    let asks_write = d.contains("编写") || d.contains("实现") || d.contains("write");
    let asks_program = d.contains("程序") || d.contains("c++") || d.contains("cpp");
    let asks_run = d.contains("执行")
        || d.contains("运行")
        || d.contains("编译")
        || d.contains("build")
        || d.contains("run");
    asks_write && asks_program && asks_run
}

fn program_build_run_has_cpp_source_artifact(result: &TaskResult) -> bool {
    result.artifacts.iter().any(|a| match a.kind {
        ArtifactKind::File => a.path.as_deref().is_some_and(|p| {
            let p = p.to_lowercase();
            p.ends_with(".cpp") || p.ends_with(".cc") || p.ends_with(".cxx")
        }),
        ArtifactKind::BuildArtifact(kind) => {
            matches!(kind, super::task::BuildArtifactKind::SourceFile)
        }
        _ => false,
    })
}

fn program_build_run_text_suggests_write(combined_lower: &str) -> bool {
    combined_lower.contains(".cpp")
        && (combined_lower.contains("create_file")
            || combined_lower.contains("已创建文件")
            || combined_lower.contains("created file")
            || combined_lower.contains("write_file")
            || combined_lower.contains("apply_patch"))
}

fn program_build_run_text_suggests_compile(combined_lower: &str) -> bool {
    combined_lower.contains("g++")
        || combined_lower.contains("clang++")
        || combined_lower.contains("编译")
        || combined_lower.contains("cmake")
        || combined_lower.contains("make")
        || combined_lower.contains("build")
}

fn program_build_run_text_suggests_executed(result: &TaskResult, combined_raw: &str) -> bool {
    result.tools_invoked.iter().any(|n| n == "run_executable")
        || (result.tools_invoked.iter().any(|n| n == "run_command")
            && run_command_invocation_mentions_hello(combined_raw))
}

fn verify_program_build_and_run_evidence(result: &TaskResult) -> Result<(), String> {
    let combined_lower = format!(
        "{}\n{}",
        result.output.as_deref().unwrap_or(""),
        result.error.as_deref().unwrap_or("")
    )
    .to_lowercase();

    let has_source_artifact = program_build_run_has_cpp_source_artifact(result);
    let wrote_source =
        has_source_artifact || program_build_run_text_suggests_write(&combined_lower);

    let compiled = program_build_run_text_suggests_compile(&combined_lower);

    let combined_raw = format!(
        "{}\n{}",
        result.output.as_deref().unwrap_or(""),
        result.error.as_deref().unwrap_or("")
    );
    let ran_program = program_build_run_text_suggests_executed(result, &combined_raw);

    let mut missing = Vec::new();
    if !wrote_source {
        missing.push("write_source");
    }
    if !compiled {
        missing.push("compile");
    }
    if !ran_program {
        missing.push("run");
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "编写并执行程序验收未通过; missing: {}; hint: 需包含写源码(.cpp)+编译(g++/clang++)+运行(可执行输出)",
            missing.join(",")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::super::task::GoalAcceptance;
    use super::*;

    #[test]
    fn test_parse_exit_code_delegates_to_shared_kernel() {
        use crate::agent::acceptance::parse_exit_code_from_combined_output;
        assert_eq!(parse_exit_code_from_combined_output("退出码：0"), Some(0));
        assert_eq!(parse_exit_code_from_combined_output("(exit=1)"), Some(1));
        assert_eq!(
            parse_exit_code_from_combined_output("exit code: 127"),
            Some(127)
        );
        assert!(parse_exit_code_from_combined_output("some output").is_none());
    }

    #[test]
    fn test_verification_pass_no_acceptance() {
        let verifier = GoalVerifier::new(std::env::temp_dir());
        let goal = SubGoal::new("test", "test goal");
        let result = TaskResult {
            task_id: "test".to_string(),
            status: TaskStatus::Completed,
            output: Some("done".to_string()),
            error: None,
            artifacts: vec![],
            duration_ms: 0,
            tools_invoked: vec![],
        };

        let verify_result = verifier.verify(&goal, &result);
        assert!(verify_result.is_pass());
    }

    #[test]
    fn test_verification_fail_task_failed() {
        let verifier = GoalVerifier::new(std::env::temp_dir());
        let goal = SubGoal::new("test", "test goal");
        let result = TaskResult {
            task_id: "test".to_string(),
            status: TaskStatus::Failed {
                reason: "error".to_string(),
            },
            output: None,
            error: Some("error".to_string()),
            artifacts: vec![],
            duration_ms: 0,
            tools_invoked: vec![],
        };

        let verify_result = verifier.verify(&goal, &result);
        assert!(verify_result.is_fail());
    }

    #[test]
    fn run_command_hello_detects_zh_stdout_block() {
        let sample = r#"
        Tool run_command executed successfully: 退出码：0
        标准输出：
        Hello, World!
        "#;
        assert!(run_command_invocation_mentions_hello(sample));
    }

    /// 与 Operator 里拼出的「单条工具观察」前缀一致，供「运行可执行体」提前收尾与验收对齐
    #[test]
    fn run_command_hello_detects_operator_observation_prefix() {
        let tool_body = "退出码：0\n标准输出：\nHello, World!\n";
        let obs = format!("Tool run_command executed successfully: {}", tool_body);
        assert!(run_command_invocation_mentions_hello(&obs));
    }

    #[test]
    fn run_command_hello_falls_back_with_trace() {
        let s = "subgoal_tool_trace\n… run_command …\n标准输出：\nHello, World!\n退出码：0\nexecuted successfully\n";
        assert!(run_command_invocation_mentions_hello(s));
    }

    #[test]
    fn run_executable_verification_failure_contains_diag_fields() {
        let result = TaskResult {
            task_id: "goal_run".to_string(),
            status: TaskStatus::Completed,
            output: Some("Tool run_command\n标准输出：\n(no hello)\n退出码：0".to_string()),
            error: None,
            artifacts: vec![],
            duration_ms: 0,
            tools_invoked: vec!["run_command".to_string()],
        };
        let err =
            verify_run_executable_subgoal_tool_evidence(&result, &[]).expect_err("should fail");
        assert!(err.contains("[diag tools_invoked="));
        assert!(err.contains("exit_code="));
        assert!(err.contains("output_preview="));
    }

    #[test]
    fn program_build_run_goal_fails_when_only_read_dir() {
        let verifier = GoalVerifier::new(std::env::temp_dir());
        let goal = SubGoal::new("goal_1", "编写一个简单c++程序并执行");
        let result = TaskResult {
            task_id: "goal_1".to_string(),
            status: TaskStatus::Completed,
            output: Some("✅ read_dir 成功: 目录: .".to_string()),
            error: None,
            artifacts: vec![],
            duration_ms: 2654,
            tools_invoked: vec!["read_dir".to_string()],
        };
        let verify_result = verifier.verify(&goal, &result);
        match verify_result {
            VerificationResult::Fail { reason } => {
                assert!(reason.contains("missing:"));
                assert!(reason.contains("write_source"));
                assert!(reason.contains("compile"));
                assert!(reason.contains("run"));
            }
            _ => panic!("expected fail with missing evidence"),
        }
    }

    #[test]
    fn build_only_subgoal_is_not_run_executable_subgoal() {
        let goal = SubGoal::new(
            "goal_build",
            "运行 cmake --build build 编译生成可执行文件，不执行程序",
        );
        assert!(!is_run_executable_subgoal(&goal));
    }

    #[test]
    fn goal_acceptance_expect_http_status_merge_output_without_http_tool_name() {
        let verifier = GoalVerifier::new(std::env::temp_dir());
        let mut goal = SubGoal::new("g", "verify http json");
        goal.acceptance = Some(GoalAcceptance {
            expect_http_status: Some(200),
            ..Default::default()
        });
        let result = TaskResult {
            task_id: "g".to_string(),
            status: TaskStatus::Completed,
            output: Some(r#"{"status": 200, "body": "ok"}"#.to_string()),
            error: None,
            artifacts: vec![],
            duration_ms: 0,
            tools_invoked: vec![],
        };
        assert!(verifier.verify(&goal, &result).is_pass());
    }

    #[test]
    fn goal_acceptance_expect_stdout_contains_is_case_sensitive() {
        let verifier = GoalVerifier::new(std::env::temp_dir());
        let mut goal = SubGoal::new("g", "case");
        goal.acceptance = Some(GoalAcceptance {
            expect_stdout_contains: Some("Ok".to_string()),
            ..Default::default()
        });
        let result = TaskResult {
            task_id: "g".to_string(),
            status: TaskStatus::Completed,
            output: Some("ok".to_string()),
            error: None,
            artifacts: vec![],
            duration_ms: 0,
            tools_invoked: vec![],
        };
        assert!(verifier.verify(&goal, &result).is_fail());
    }

    #[test]
    fn run_subgoal_is_run_executable_subgoal() {
        let goal = SubGoal::new(
            "goal_run",
            "运行 ./build/demo_app 并验证输出包含 EXPECTED_MARKER",
        );
        assert!(is_run_executable_subgoal(&goal));
    }

    #[test]
    fn build_subgoal_gets_default_exit_code_when_acceptance_missing() {
        let verifier = GoalVerifier::new(std::env::temp_dir());
        let goal = SubGoal::new("g", "在本工作区运行 cargo build 并确认通过");
        let result = TaskResult {
            task_id: "g".to_string(),
            status: TaskStatus::Completed,
            output: Some("退出码：1\n编译失败".to_string()),
            error: None,
            artifacts: vec![],
            duration_ms: 0,
            tools_invoked: vec!["run_command".to_string()],
        };
        let verify_result = verifier.verify(&goal, &result);
        assert!(verify_result.is_fail());
    }
}
