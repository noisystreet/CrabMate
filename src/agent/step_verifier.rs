//! 确定性验证闸门（StepVerifier）：根据 `PlanStepV1` 中的 `acceptance` 对执行结果进行硬断言。
//!
//! 支持的验证规则：
//! - `expect_exit_code`：退出码验证（如 `cargo test` → 0）
//! - `expect_stdout_contains`：stdout 是否包含指定字符串
//! - `expect_stderr_contains`：stderr 是否包含指定字符串
//! - `expect_file_exists`：文件是否存在
//! - `expect_json_path_equals`：JSON path 验证
//! - `expect_http_status`：HTTP 状态码验证（仅对 http_request/fetch 类工具）

use crate::agent::plan_artifact::{JsonPathEqualsRule, PlanStepAcceptance};
use crate::tool_result::ToolError;
use crate::types::Message;

#[derive(Debug)]
pub enum VerifyResult {
    Pass,
    Fail { reason: String },
}

impl VerifyResult {
    pub fn is_pass(&self) -> bool {
        matches!(self, VerifyResult::Pass)
    }

    pub fn unwrap_or_pass(self) {
        // 用于忽略失败结果的场景
    }
}

/// 从工具输出中提取 JSON 信封内的 output 字段（如果有的话）
fn extract_json_output(output: &str) -> Option<String> {
    let trimmed = output.trim();
    if !trimmed.starts_with('{') {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(trimmed).ok()?;
    let env = parsed.get("crabmate_tool")?;
    let output_val = env.get("output")?;
    Some(output_val.as_str()?.to_string())
}

/// 尝试从输出中解析 HTTP 状态码（用于 http_request/http_fetch 类工具）
fn extract_http_status(tool_name: &str, output: &str) -> Option<u16> {
    let normalized = extract_json_output(output).unwrap_or_else(|| output.to_string());

    // 尝试从 JSON 输出中提取 HTTP 状态码
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&normalized) {
        // 常见字段：status, status_code, http_status
        for field in ["status", "status_code", "http_status", "httpStatusCode"] {
            if let Some(val) = v.get(field)
                && let Some(code) = val.as_u64()
            {
                return Some(code as u16);
            }
        }
    }

    // 尝试从纯文本输出中匹配 "HTTP/1.1 200 OK" 或 "状态码：200" 等模式
    for line in normalized.lines() {
        let line = line.trim();
        if line.starts_with("HTTP/") {
            // 匹配 "HTTP/1.1 200 OK" 或 "HTTP/2 200"
            if let Some(pos) = line.find(' ') {
                let status_part = &line[pos + 1..];
                if let Some(first_token) = status_part.split_whitespace().next()
                    && let Ok(code) = first_token.parse::<u16>()
                {
                    return Some(code);
                }
            }
        }
        if line.contains("状态码") {
            // 提取数字
            let digits: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                return Some(code);
            }
        }
    }

    // 对于 http_request/http_fetch 工具，根据退出码推断
    if tool_name.contains("http") {
        // 尝试从 "退出码：0" 模式判断成功
        if output.contains("退出码：0") || output.contains("(exit=0)") {
            return Some(200);
        }
    }

    None
}

/// 简单的 JSON path 解析器，支持 `$.field.nested` 和 `$[0].field` 格式
fn get_json_path_value(json_str: &str, path: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let mut current = &value;
    let parts: Vec<&str> = path.split('.').collect();

    for (i, part) in parts.iter().enumerate() {
        let part = *part;

        // 处理数组索引 $[0] 或 $[0].field
        if part.starts_with('$') && i == 0 {
            // 去掉 $ 前缀继续
            let rest = &part[1..];
            if rest.is_empty() {
                continue;
            }
            // 继续处理
            if let Some((idx, next)) = rest.split_once('[') {
                if !idx.is_empty() {
                    // 顶级属性访问
                    current = current.get(idx)?;
                }
                let idx_str = next.trim_start_matches('[').trim_end_matches(']');
                if let Ok(index) = idx_str.parse::<usize>() {
                    current = current.get(index)?;
                }
            } else if !rest.is_empty() {
                current = current.get(rest)?;
            }
            continue;
        }

        // 处理数组索引
        if let Some((field, rest)) = part.split_once('[') {
            if !field.is_empty() {
                current = current.get(field)?;
            }
            let idx_str = rest.trim_start_matches('[').trim_end_matches(']');
            if let Ok(index) = idx_str.parse::<usize>() {
                current = current.get(index)?;
            }
            // 处理数组后面的字段访问
            if let Some((_, next_field)) = rest.split_once(']').and_then(|(_, b)| b.split_once('.'))
                && !next_field.is_empty()
            {
                current = current.get(next_field)?;
            }
            continue;
        }

        // 普通字段访问
        if !part.is_empty() {
            current = current.get(part)?;
        }
    }

    Some(current.clone())
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
        return VerifyResult::Fail {
            reason: "Step verification failed: no tool execution result found in messages"
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

/// 对单个步骤的工具执行结果进行验证
fn verify_tool_execution_inner(
    acceptance: &PlanStepAcceptance,
    tool_name: &str,
    tool_output: &str,
    stdout: &str,
    stderr: &str,
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
                    "exit_code_mismatch: expected {}, got {}",
                    expected_code, actual_code
                ),
            };
        }
    }

    // 2. 检查 stdout 是否包含指定的字符串
    if let Some(ref expect_str) = acceptance.expect_stdout_contains
        && !stdout.contains(expect_str)
    {
        return VerifyResult::Fail {
            reason: format!("stdout_missing: expected to contain '{}'", expect_str),
        };
    }

    // 3. 检查 stderr 是否包含指定的字符串
    if let Some(ref expect_str) = acceptance.expect_stderr_contains
        && !stderr.contains(expect_str)
    {
        return VerifyResult::Fail {
            reason: format!("stderr_missing: expected to contain '{}'", expect_str),
        };
    }

    // 4. 检查文件是否存在
    if let Some(ref file_path) = acceptance.expect_file_exists {
        let resolved =
            crate::path_workspace::absolutize_relative_under_root(workspace_root, file_path);
        match resolved {
            Ok(p) if p.exists() => {
                // file exists, ok
            }
            Ok(_) => {
                return VerifyResult::Fail {
                    reason: format!("file_not_found: '{}'", file_path),
                };
            }
            Err(_) => {
                return VerifyResult::Fail {
                    reason: format!("file_path_invalid: '{}'", file_path),
                };
            }
        }
    }

    // 5. JSON path 验证
    if let Some(ref json_rule) = acceptance.expect_json_path_equals {
        let JsonPathEqualsRule {
            path,
            value: expected,
        } = json_rule;

        // 尝试从 tool_output 解析 JSON
        let json_output =
            extract_json_output(tool_output).unwrap_or_else(|| tool_output.to_string());

        match get_json_path_value(&json_output, path) {
            Some(actual) => {
                if &actual != expected {
                    return VerifyResult::Fail {
                        reason: format!(
                            "json_path_mismatch: path '{}' expected {}, got {}",
                            path, expected, actual
                        ),
                    };
                }
            }
            None => {
                return VerifyResult::Fail {
                    reason: format!(
                        "json_path_error: could not extract value at path '{}'",
                        path
                    ),
                };
            }
        }
    }

    // 6. HTTP 状态码验证（仅对 http_request/http_fetch 类工具生效）
    if let Some(expected_status) = acceptance.expect_http_status {
        let tool_name_lower = tool_name.to_lowercase();
        if tool_name_lower.contains("http") || tool_name_lower.contains("fetch") {
            if let Some(actual_status) = extract_http_status(tool_name, tool_output) {
                if actual_status != expected_status {
                    return VerifyResult::Fail {
                        reason: format!(
                            "http_status_mismatch: expected {}, got {}",
                            expected_status, actual_status
                        ),
                    };
                }
            } else {
                // 无法提取状态码时，检查退出码作为后备
                let exit_code = tool_error.and_then(|e| e.code.parse::<i32>().ok());
                if exit_code == Some(0) && (200..300).contains(&expected_status) {
                    // exit 0 对于 HTTP 工具通常表示成功
                } else if exit_code.is_none() {
                    return VerifyResult::Fail {
                        reason: format!(
                            "http_status_error: could not extract HTTP status code from output (expected {})",
                            expected_status
                        ),
                    };
                }
            }
        }
    }

    VerifyResult::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

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
        if let VerifyResult::Fail { reason } = result {
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
    fn test_extract_http_status_from_json_envelope() {
        let output = serde_json::json!({
            "crabmate_tool": {
                "v": 1,
                "name": "http_request",
                "output": "{\"status\": 200, \"data\": []}"
            }
        })
        .to_string();

        let status = extract_http_status("http_request", &output);
        assert_eq!(status, Some(200));
    }
}
