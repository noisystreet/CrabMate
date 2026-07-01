//! 确定性验证闸门（StepVerifier）：根据 `PlanStepV1` 中的 `acceptance` 对执行结果进行硬断言。
//!
//! **分阶段**路径下，验收针对当前分步的 **`acceptance`**（步界：分步 `user` 之后至下一真实 user / 下一条分步注入；步内编排注入 user 不截断）。
//! 空规范直接 **Pass**；仅 **`expect_file_exists`** 时查工作区、**不要求**本步 `role: tool`；其余规则在本步窗口内**自后向前**逐条 `role: tool` 尝试，**任一**满足即 **Pass**。
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

use crate::agent::acceptance::{
    AcceptanceEvidence, AcceptanceSpec, VerifyOutcome, verify_plan_step_acceptance_for_tool_message,
};
use crate::agent::plan_artifact::PlanStepAcceptance;
use crate::tool_result::ToolError;
use crate::types::{Message, tool_messages_in_staged_step_window};

pub type VerifyResult = VerifyOutcome;

fn verify_tool_message_against_acceptance(
    acceptance: &PlanStepAcceptance,
    tool_msg: &Message,
    workspace_root: &std::path::Path,
) -> VerifyResult {
    verify_plan_step_acceptance_for_tool_message(acceptance, tool_msg, workspace_root)
}

/// 对**分阶段单步**内的工具结果进行验证（`step_user_index` 为本步分步 `user` 在 `messages` 中的下标）。
pub fn verify_step_execution(
    acceptance: &PlanStepAcceptance,
    messages: &[Message],
    step_user_index: usize,
    workspace_root: &std::path::Path,
) -> VerifyResult {
    let spec = AcceptanceSpec::from(acceptance);
    if spec.is_empty() {
        return VerifyOutcome::Pass;
    }

    if !spec.requires_tool_evidence() {
        let ev = AcceptanceEvidence {
            tool_name: "",
            tool_output: "",
            stdout: "",
            stderr: "",
            tool_error: None,
            fallback_exit_code: None,
            workspace_root,
            file_resolve: spec.file_resolve,
            combined_text_override: None,
        };
        return crate::agent::acceptance::verify_against_spec(&spec, &ev);
    }

    let tools = tool_messages_in_staged_step_window(messages, step_user_index);
    if tools.is_empty() {
        return VerifyOutcome::Fail {
            reason: "Step verification failed: no tool result in this staged step (after step user, before next user message)"
                .to_string(),
        };
    }

    let mut last_fail: Option<VerifyOutcome> = None;
    for tool_msg in tools.iter().rev() {
        match verify_tool_message_against_acceptance(acceptance, tool_msg, workspace_root) {
            VerifyOutcome::Pass => return VerifyOutcome::Pass,
            fail @ VerifyOutcome::Fail { .. } => last_fail = Some(fail),
        }
    }
    last_fail.unwrap_or(VerifyOutcome::Fail {
        reason: "Step verification failed: no tool result in this staged step satisfied acceptance"
            .to_string(),
    })
}

/// 对单个步骤的工具执行结果进行验证（供测试与内部复用）。
#[allow(dead_code)] // 生产路径经 `verify_plan_step_acceptance_for_tool_message`；单测仍直接调用
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

    #[test]
    fn stdout_contains_matches_archive_unpack_tool_output() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: Some("已解压".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };

        let result = verify_tool_execution_inner(
            &acceptance,
            "archive_unpack",
            "已解压 187 个文件到: .",
            "",
            "",
            None,
            std::path::Path::new("/tmp"),
        );

        assert!(result.is_pass());
    }

    /// 分阶段：步窗口内自后向前聚合 tool；前序 build 成功、末条 probe 失败时仍 Pass。
    #[test]
    fn verify_step_passes_when_earlier_tool_satisfies_acceptance_not_last_probe() {
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
        let messages = vec![
            Message::user_only("### 分步 1/1"),
            t_step(0, "[100%] Built target hello"),
            t_step(1, "probe-only"),
        ];
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: Some("Built target hello".to_string()),
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let r = verify_step_execution(&acceptance, &messages, 0, std::path::Path::new("/tmp"));
        assert!(r.is_pass());
    }

    /// 分阶段：验收只读「本步 / 自 step_user 至下一 user 之间」的 tool 窗口；若误用**全局**最后一条，会在后续 `user` 之后仍错判为最后 tool。
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
    fn verify_step_empty_acceptance_passes_without_tool() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let messages = vec![
            Message::user_only("### 分步 1/1"),
            Message::assistant_only("仅文字结论，无工具"),
        ];
        let r = verify_step_execution(&acceptance, &messages, 0, std::path::Path::new("/tmp"));
        assert!(r.is_pass());
    }

    #[test]
    fn verify_step_file_exists_only_without_tool() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("微积分.rst");
        std::fs::write(&file, "ok").expect("write");

        let acceptance = PlanStepAcceptance {
            expect_exit_code: None,
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: Some("微积分.rst".to_string()),
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let messages = vec![
            Message::user_only("### 分步 1/1"),
            Message::assistant_only("结论"),
        ];
        let r = verify_step_execution(&acceptance, &messages, 0, dir.path());
        assert!(r.is_pass());
    }

    #[test]
    fn verify_step_exit_code_requires_tool_in_step_window() {
        let acceptance = PlanStepAcceptance {
            expect_exit_code: Some(0),
            expect_stdout_contains: None,
            expect_stderr_contains: None,
            expect_file_exists: None,
            expect_json_path_equals: None,
            expect_http_status: None,
        };
        let messages = vec![
            Message::user_only("### 分步 1/1"),
            Message::assistant_only("无工具"),
        ];
        let r = verify_step_execution(&acceptance, &messages, 0, std::path::Path::new("/tmp"));
        assert!(!r.is_pass());
        if let VerifyOutcome::Fail { reason } = r {
            assert!(reason.contains("no tool result"));
        }
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
