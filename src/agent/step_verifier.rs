//! 确定性验证闸门（StepVerifier）：根据 `PlanStepV1` 中的 `acceptance` 对执行结果进行硬断言。

use crate::agent::plan_artifact::PlanStepAcceptance;
use crate::tool_result::ToolError;
use crate::types::Message;

pub enum VerifyResult {
    Pass,
    Fail { reason: String },
}

/// 对步骤内的工具执行历史进行验证
pub fn verify_step_execution(
    acceptance: &PlanStepAcceptance,
    messages: &[Message],
    workspace_root: &std::path::Path,
) -> VerifyResult {
    // 找到本步注入的最后一个工具执行结果
    let last_tool_msg = messages.iter().rev().find(|m| m.role == "tool");
    let (tool_name, tool_output) = if let Some(tool_msg) = last_tool_msg {
        let name = tool_msg.name.as_deref().unwrap_or("");
        let output = crate::types::message_content_as_str(&tool_msg.content).unwrap_or("");
        (name, output)
    } else {
        ("", "")
    };

    let mut tool_error_opt = None;
    let parsed = crate::tool_result::parse_legacy_output(tool_name, tool_output);
    let mut fake_error = crate::tool_result::ToolError {
        code: String::new(),
        category: crate::tool_result::ToolFailureCategory::External,
        message: "Verification fake error".to_string(),
        legacy_parsed: parsed.clone(),
        retryable: false,
    };
    if let Some(code) = parsed.exit_code {
        fake_error.code = code.to_string();
        tool_error_opt = Some(fake_error);
    }

    verify_tool_execution_inner(
        acceptance,
        tool_name,
        tool_output,
        tool_error_opt.as_ref(),
        workspace_root,
    )
}

/// 对单个步骤的工具执行结果进行验证
fn verify_tool_execution_inner(
    acceptance: &PlanStepAcceptance,
    _tool_name: &str,
    tool_output: &str,
    tool_error: Option<&ToolError>,
    workspace_root: &std::path::Path,
) -> VerifyResult {
    // 1. 检查退出码（例如针对 `cargo_test` 或 `run_command`）
    if let Some(expected_code) = acceptance.expect_exit_code {
        let actual_code = tool_error
            .and_then(|e| e.code.parse::<i32>().ok())
            .unwrap_or(0);
        if actual_code != expected_code {
            return VerifyResult::Fail {
                reason: format!(
                    "Step verification failed: expected exit code {}, but got {}",
                    expected_code, actual_code
                ),
            };
        }
    }

    // 2. 检查标准输出（或执行结果正文）是否包含指定的字符串
    #[allow(clippy::collapsible_if)]
    if let Some(ref expect_str) = acceptance.expect_stdout_contains {
        if !tool_output.contains(expect_str) {
            return VerifyResult::Fail {
                reason: format!(
                    "Step verification failed: output does not contain expected string '{}'",
                    expect_str
                ),
            };
        }
    }

    // 3. 检查文件是否存在
    if let Some(ref file_path) = acceptance.expect_file_exists {
        // 使用 path_workspace 解析路径
        let resolved =
            crate::path_workspace::absolutize_relative_under_root(workspace_root, file_path);
        match resolved {
            Ok(p) if p.exists() => {
                // file exists, ok
            }
            Ok(_) => {
                return VerifyResult::Fail {
                    reason: format!(
                        "Step verification failed: expected file '{}' does not exist",
                        file_path
                    ),
                };
            }
            Err(_) => {
                return VerifyResult::Fail {
                    reason: format!(
                        "Step verification failed: expected file path '{}' is invalid or out of bounds",
                        file_path
                    ),
                };
            }
        }
    }

    VerifyResult::Pass
}
