//! 确定性验证闸门（StepVerifier）：根据 `PlanStepV1` 中的 `acceptance` 对执行结果进行硬断言。
//!
//! **分阶段**路径下，验收只针对**当前步**：自该步分步 `user` 消息下标之后、**下一条** `user` 或会话末尾之前，取**最后一条** `role: tool` 消息（与
//! 分阶段对「步内工具是否成功」的扫描范围一致）。若一步内调用了多种工具，通常应将验收条件对准**本步最后一条**工具输出，或拆成多步、每步一次工具加验收。
//!
//! 核心判定逻辑见 [`crate::agent::acceptance`]。
//!
//! 支持的验证规则：
//! - `expect_exit_code`：退出码验证（如 `cargo test` → 0）
//! - `expect_stdout_contains`：stdout 是否包含指定字符串
//! - `expect_stderr_contains`：stderr 是否包含指定字符串
//! - `expect_file_exists`：文件是否存在
//! - `expect_json_path_equals`：JSON path 验证
//! - `expect_http_status`：HTTP 状态码验证（仅对 http_request/fetch 类工具）

use crate::agent::acceptance::{AcceptanceEvidence, AcceptanceSpec, VerifyOutcome};
use crate::agent::plan_artifact::PlanStepAcceptance;
use crate::tool_result::ToolError;
use crate::types::Message;

pub type VerifyResult = VerifyOutcome;

/// 自 `step_user_index` 指向的分步 `user` 起，到下一 `user` 或 `messages` 末尾为止，取其中**最后**一条 `role: tool`（分阶段步界内的「验收用」工具条）。
fn last_tool_message_in_staged_step<'a>(
    messages: &'a [Message],
    step_user_index: usize,
) -> Option<&'a Message> {
    if step_user_index >= messages.len() {
        return None;
    }
    let mut i = step_user_index.saturating_add(1);
    let mut last_tool: Option<&'a Message> = None;
    while i < messages.len() {
        let m = &messages[i];
        if m.role == "user" {
            break;
        }
        if m.role == "tool" {
            last_tool = Some(m);
        }
        i += 1;
    }
    last_tool
}

/// 对**分阶段单步**内的工具结果进行验证（`step_user_index` 为本步分步 `user` 在 `messages` 中的下标）。
pub fn verify_step_execution(
    acceptance: &PlanStepAcceptance,
    messages: &[Message],
    step_user_index: usize,
    workspace_root: &std::path::Path,
) -> VerifyResult {
    let last_tool_msg = last_tool_message_in_staged_step(messages, step_user_index);
    let (tool_name, tool_output) = if let Some(tool_msg) = last_tool_msg {
        let name = tool_msg.name.as_deref().unwrap_or("");
        let output = crate::types::message_content_as_str(&tool_msg.content).unwrap_or("");
        (name, output)
    } else {
        return VerifyOutcome::Fail {
            reason: "Step verification failed: no tool result in this staged step (after step user, before next user message)"
                .to_string(),
        };
    };

    let parsed = crate::tool_result::parse_legacy_output(tool_name, tool_output);

    let tool_error_opt = parsed.exit_code.map(|code| crate::tool_result::ToolError {
        code: code.to_string(),
        category: crate::tool_result::ToolFailureCategory::External,
        message: "Verification fake error".to_string(),
        legacy_parsed: parsed.clone(),
        retryable: false,
    });

    verify_tool_execution_inner(
        acceptance,
        tool_name,
        tool_output,
        parsed.stdout.as_str(),
        parsed.stderr.as_str(),
        tool_error_opt.as_ref(),
        workspace_root,
    )
}

/// 对单个步骤的工具执行结果进行验证（供测试与内部复用）。
pub(crate) fn verify_tool_execution_inner(
    acceptance: &PlanStepAcceptance,
    tool_name: &str,
    tool_output: &str,
    stdout: &str,
    stderr: &str,
    tool_error: Option<&ToolError>,
    workspace_root: &std::path::Path,
) -> VerifyResult {
    let spec = AcceptanceSpec::from(acceptance);
    let ev = AcceptanceEvidence {
        tool_name,
        tool_output,
        stdout,
        stderr,
        tool_error,
        fallback_exit_code: None,
        workspace_root,
        file_resolve: spec.file_resolve,
        combined_text_override: None,
    };
    crate::agent::acceptance::verify_against_spec(&spec, &ev)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::plan_artifact::JsonPathEqualsRule;

    #[test]
    fn test_exit_code_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };

        let fake_error = ToolError {
            code: "0".to_string(),
            category: crate::tool_result::ToolFailureCategory::External,
            message: "ok".to_string(),
            legacy_parsed: crate::tool_result::ParsedLegacyOutput {
                ok: true,
                exit_code: Some(0),
                stdout: String::new(),
                stderr: String::new(),
                error_code: None,
            },
            retryable: false,
        };

        let result = verify_tool_execution_inner(
            &acceptance,
            "cargo_test",
            "",
            "",
            "",
            Some(&fake_error),
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_exit_code_fail() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };

        let fake_error = ToolError {
            code: "1".to_string(),
            category: crate::tool_result::ToolFailureCategory::External,
            message: "failed".to_string(),
            legacy_parsed: crate::tool_result::ParsedLegacyOutput {
                ok: false,
                exit_code: Some(1),
                stdout: String::new(),
                stderr: String::new(),
                error_code: Some("test_failed".to_string()),
            },
            retryable: false,
        };

        let result = verify_tool_execution_inner(
            &acceptance,
            "cargo_test",
            "",
            "",
            "",
            Some(&fake_error),
            std::path::Path::new("/tmp"),
        );

        assert!(!result.is_pass());
        if let VerifyOutcome::Fail { reason } = result {
            assert!(reason.contains("exit_code_mismatch"));
        }
    }

    #[test]
    fn test_stdout_contains_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: Some("passed".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };

        let result = verify_tool_execution_inner(
            &acceptance,
            "cargo_test",
            "",               // tool_output
            "2 tests passed", // stdout
            "",               // stderr
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_stderr_contains_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: Some("warning:".to_string()),
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };

        let result = verify_tool_execution_inner(
            &acceptance,
            "cargo_build",
            "",
            "compiling...",
            "warning: unused variable",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_json_path_equals_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: Some(JsonPathEqualsRule {
                path: "$.status".to_string(),
                value: serde_json::json!("ok"),
            }),
            expect_http_status: None,
        };

        let output = r#"{"status": "ok", "data": "hello"}"#;
        let result = verify_tool_execution_inner(
            &acceptance,
            "http_request",
            output,
            output,
            "",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_json_path_nested_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: Some(JsonPathEqualsRule {
                path: "$.data.items[0].name".to_string(),
                value: serde_json::json!("item1"),
            }),
            expect_http_status: None,
        };

        let output = r#"{"data":{"items":[{"name":"item1","value":100}]}}"#;
        let result = verify_tool_execution_inner(
            &acceptance,
            "http_request",
            output,
            output,
            "",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_json_path_array_index() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: Some(JsonPathEqualsRule {
                path: "$[1]".to_string(),
                value: serde_json::json!("second"),
            }),
            expect_http_status: None,
        };

        let output = r#"["first", "second", "third"]"#;
        let result = verify_tool_execution_inner(
            &acceptance,
            "http_fetch",
            output,
            output,
            "",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_http_status_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: Some(200),
        };

        let output = r#"{"status": 200, "body": "ok"}"#;
        let result = verify_tool_execution_inner(
            &acceptance,
            "http_request",
            output,
            output,
            "",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_http_status_from_envelope() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: Some(201),
        };

        let output = serde_json::json!({
            "crabmate_tool": {
                "v": 1,
                "name": "http_request",
                "output": "{\"status\": 201, \"id\": 123}"
            }
        })
        .to_string();

        let result = verify_tool_execution_inner(
            &acceptance,
            "http_request",
            &output,
            &output,
            "",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    #[test]
    fn test_multiple_rules_all_pass() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: Some("test".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: Some(JsonPathEqualsRule {
                path: "$.ok".to_string(),
                value: serde_json::json!(true),
            }),
            expect_http_status: None,
        };

        let fake_error = ToolError {
            code: "0".to_string(),
            category: crate::tool_result::ToolFailureCategory::External,
            message: "ok".to_string(),
            legacy_parsed: crate::tool_result::ParsedLegacyOutput {
                ok: true,
                exit_code: Some(0),
                stdout: "all tests passed".to_string(),
                stderr: String::new(),
                error_code: None,
            },
            retryable: false,
        };

        let output = r#"{"ok": true}"#;
        let result = verify_tool_execution_inner(
            &acceptance,
            "cargo_test",
            output,
            "all tests passed",
            "",
            Some(&fake_error),
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    /// 分阶段：验收只读「本步 / 自 step_user 至下一 user 之间」最后一条 `tool`；若误用**全局**最后一条，会在后续 `user` 之后仍错判为最后 tool。
    #[test]
    fn verify_step_uses_last_tool_in_step_window_not_last_tool_globally() {
        use crate::types::Message;
        use crate::types::MessageContent;

        let t_step = |exit: i32, stdout: &str| {
            let body = if stdout.is_empty() {
                format!("退出码：{exit}\n")
            } else {
                format!("退出码：{exit}\n标准输出：\n{stdout}\n")
            };
            Message {
                role: "tool".to_string(),
                content: Some(MessageContent::Text(body)),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: Some("run_command".to_string()),
                tool_call_id: None,
            }
        };
        // step_user @0：步内 t1(0) + t2(1)；下一 user 之后 t3(0)（模拟后续步）
        let messages = vec![
            Message::user_only("### 分步 1/1"),
            t_step(0, "alpha"),
            t_step(1, "beta-expected"), // 本步最后 tool
            Message::user_only("next block"),
            t_step(0, "gamma-wrong"), // 全局 last tool；不得用于本步
        ];
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(1),
            expect_stdout_contains: Some("beta-expected".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let r = verify_step_execution(&acceptance, &messages, 0, std::path::Path::new("/tmp"));
        assert!(r.is_pass());
    }

    #[test]
    fn test_extract_http_status_from_json_envelope() {
        let output = serde_json::json!({
            "crabmate_tool": {
                "v": 1,
                "name": "http_request",
                "output": "{\"status\": 200, \"data\": []}"
            }
        })
        .to_string();

        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: Some(200),
        };
        let r = verify_tool_execution_inner(
            &acceptance,
            "http_request",
            &output,
            &output,
            "",
            None,
            std::path::Path::new("/tmp"),
        );
        assert!(r.is_pass());
    }
}
