//! 子目标验证器（GoalVerifier）：验证子目标是否达成
//!
//! 根据 SubGoal 中定义的 acceptance 条件对执行结果进行验证

use super::task::{ArtifactKind, GoalAcceptance, SubGoal, TaskResult, TaskStatus};

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
    /// 2. 文件存在性
    /// 3. 输出内容匹配
    /// 4. 验证命令执行
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

        // 如果没有定义验收条件，直接通过
        let acceptance = match &goal.acceptance {
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

        // 验证文件存在性
        if let Err(reason) = self.verify_file_exists(acceptance) {
            return VerificationResult::Fail { reason };
        }

        // 验证输出内容
        if let Err(reason) = self.verify_output_contains(acceptance, result) {
            return VerificationResult::Fail { reason };
        }

        // 验证退出码
        if let Err(reason) = self.verify_exit_code(acceptance, result) {
            return VerificationResult::Fail { reason };
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

    /// 验证文件是否存在
    fn verify_file_exists(&self, acceptance: &GoalAcceptance) -> Result<(), String> {
        for path in &acceptance.expect_file_exists {
            let full_path = self.workspace_root.join(path);
            if !full_path.exists() {
                return Err(format!("期望文件不存在: {}", path));
            }
            log::debug!(
                target: "crabmate",
                "[GOAL_VERIFIER] File exists: {}",
                full_path.display()
            );
        }
        Ok(())
    }

    /// 验证输出内容是否包含期望字符串
    fn verify_output_contains(
        &self,
        acceptance: &GoalAcceptance,
        result: &TaskResult,
    ) -> Result<(), String> {
        if acceptance.expect_output_contains.is_empty() {
            return Ok(());
        }

        let output = result.output.as_deref().unwrap_or("").to_lowercase();
        let error = result.error.as_deref().unwrap_or("").to_lowercase();
        let combined = format!("{} {}", output, error);

        for expected in &acceptance.expect_output_contains {
            if !combined.contains(&expected.to_lowercase()) {
                return Err(format!("输出不包含期望内容: '{}'", expected));
            }
            log::debug!(
                target: "crabmate",
                "[GOAL_VERIFIER] Output contains: {}",
                expected
            );
        }
        Ok(())
    }

    /// 验证退出码
    fn verify_exit_code(
        &self,
        acceptance: &GoalAcceptance,
        result: &TaskResult,
    ) -> Result<(), String> {
        let expected_code = match acceptance.expect_exit_code {
            Some(code) => code,
            None => return Ok(()), // 没有定义退出码要求，跳过
        };

        // 从输出中解析退出码
        // 格式如："退出码：0" 或 "(exit=0)"
        let output = result.output.as_deref().unwrap_or("");
        let error = result.error.as_deref().unwrap_or("");
        let combined = format!("{} {}", output, error);

        // 尝试提取退出码
        let exit_code = Self::extract_exit_code(&combined);

        match exit_code {
            Some(code) if code == expected_code => {
                log::debug!(
                    target: "crabmate",
                    "[GOAL_VERIFIER] Exit code matches: {}",
                    code
                );
                Ok(())
            }
            Some(code) => Err(format!(
                "退出码不匹配: 期望 {}, 实际 {}",
                expected_code, code
            )),
            None => {
                // 无法解析退出码，假设成功（因为 TaskStatus 是 Completed）
                log::warn!(
                    target: "crabmate",
                    "[GOAL_VERIFIER] Could not extract exit code from output"
                );
                Ok(())
            }
        }
    }

    /// 从输出中提取退出码
    fn extract_exit_code(output: &str) -> Option<i32> {
        // 匹配 "退出码：0" 或 "(exit=0)" 或 "exit code: 0"
        let patterns = ["退出码：", "exit=", "exit code: ", "exit code:", "(exit="];

        for pattern in &patterns {
            if let Some(pos) = output.find(pattern) {
                let start = pos + pattern.len();
                let rest = &output[start..];
                // 提取数字
                let num_str: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_digit() || *c == '-')
                    .collect();
                if let Ok(code) = num_str.parse::<i32>() {
                    return Some(code);
                }
            }
        }

        None
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

fn verify_program_build_and_run_evidence(result: &TaskResult) -> Result<(), String> {
    let combined = format!(
        "{}\n{}",
        result.output.as_deref().unwrap_or(""),
        result.error.as_deref().unwrap_or("")
    )
    .to_lowercase();

    let has_source_artifact = result.artifacts.iter().any(|a| match a.kind {
        ArtifactKind::File => a.path.as_deref().is_some_and(|p| {
            let p = p.to_lowercase();
            p.ends_with(".cpp") || p.ends_with(".cc") || p.ends_with(".cxx")
        }),
        ArtifactKind::BuildArtifact(kind) => {
            matches!(kind, super::task::BuildArtifactKind::SourceFile)
        }
        _ => false,
    });
    let wrote_source = has_source_artifact
        || combined.contains(".cpp")
            && (combined.contains("create_file")
                || combined.contains("已创建文件")
                || combined.contains("created file")
                || combined.contains("write_file")
                || combined.contains("apply_patch"));

    let compiled = combined.contains("g++")
        || combined.contains("clang++")
        || combined.contains("编译")
        || combined.contains("cmake")
        || combined.contains("make")
        || combined.contains("build");

    let ran_program = combined.contains("./")
        || combined.contains("运行")
        || combined.contains("执行程序")
        || combined.contains("program output")
        || combined.contains("hello");

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
    use super::*;

    #[test]
    fn test_extract_exit_code() {
        assert_eq!(GoalVerifier::extract_exit_code("退出码：0"), Some(0));
        assert_eq!(GoalVerifier::extract_exit_code("(exit=1)"), Some(1));
        assert_eq!(GoalVerifier::extract_exit_code("exit code: 127"), Some(127));
        assert_eq!(GoalVerifier::extract_exit_code("some output"), None);
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
        };

        let verify_result = verifier.verify(&goal, &result);
        assert!(verify_result.is_fail());
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
}
