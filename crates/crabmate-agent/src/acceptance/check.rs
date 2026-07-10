//! 对 [`super::AcceptanceSpec`] 与 [`super::AcceptanceEvidence`] 执行逐项断言。

use super::json_path_resolve::resolve_json_path_value;
use super::{AcceptanceEvidence, AcceptanceSpec, ExitCodePolicy, FileResolveKind, VerifyOutcome};

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

fn http_status_from_json_object(v: &serde_json::Value) -> Option<u16> {
    for field in ["status", "status_code", "http_status", "httpStatusCode"] {
        if let Some(val) = v.get(field)
            && let Some(code) = val.as_u64()
        {
            return Some(code as u16);
        }
    }
    None
}

fn http_status_from_text_lines(normalized: &str) -> Option<u16> {
    for line in normalized.lines() {
        let line = line.trim();
        if line.starts_with("HTTP/")
            && let Some(pos) = line.find(' ')
        {
            let status_part = &line[pos + 1..];
            if let Some(first_token) = status_part.split_whitespace().next()
                && let Ok(code) = first_token.parse::<u16>()
            {
                return Some(code);
            }
        }
        if line.contains("状态码") {
            let digits: String = line.chars().filter(|c| c.is_ascii_digit()).collect();
            if let Ok(code) = digits.parse::<u16>() {
                return Some(code);
            }
        }
    }
    None
}

/// 尝试从输出中解析 HTTP 状态码（用于 http_request/http_fetch 类工具）
fn extract_http_status(tool_name: &str, output: &str) -> Option<u16> {
    let normalized = extract_json_output(output).unwrap_or_else(|| output.to_string());

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&normalized)
        && let Some(code) = http_status_from_json_object(&v)
    {
        return Some(code);
    }

    if let Some(code) = http_status_from_text_lines(&normalized) {
        return Some(code);
    }

    if tool_name.contains("http") && (output.contains("退出码：0") || output.contains("(exit=0)"))
    {
        return Some(200);
    }

    None
}

fn resolve_file(workspace_root: &std::path::Path, path: &str, kind: FileResolveKind) -> bool {
    match kind {
        FileResolveKind::AbsolutizeRelative => {
            match crabmate_tools::workspace::path::absolutize_relative_under_root(
                workspace_root,
                path,
            ) {
                Ok(p) => p.exists(),
                Err(_) => false,
            }
        }
        FileResolveKind::WorkspaceJoin => workspace_root.join(path).exists(),
    }
}

fn combined_haystack(ev: &AcceptanceEvidence<'_>) -> String {
    if let Some(o) = ev.combined_text_override {
        return o.to_string();
    }
    format!("{} {}", ev.stdout, ev.stderr)
}

/// `run_command` 的 stdout/stderr 由解析器填入；其它工具（如 `archive_unpack`）正文在 `tool_output`。
fn haystack_for_stream_contains<'a>(
    ev: &'a AcceptanceEvidence<'_>,
    stream: &'a str,
    tool_output_fallback: &'a str,
) -> &'a str {
    if !stream.is_empty() {
        return stream;
    }
    if ev.tool_name == "run_command" {
        return stream;
    }
    if !tool_output_fallback.is_empty() {
        return tool_output_fallback;
    }
    stream
}

fn verify_exit_code_branch(
    spec: &AcceptanceSpec,
    ev: &AcceptanceEvidence<'_>,
) -> Option<VerifyOutcome> {
    let expected_code = spec.expect_exit_code?;

    let parsed_structured = ev
        .tool_error
        .and_then(|e| e.code.parse::<i32>().ok())
        .or(ev.fallback_exit_code);

    match parsed_structured {
        Some(actual) if actual != expected_code => Some(VerifyOutcome::Fail {
            reason: format!(
                "exit_code_mismatch: expected {}, got {}",
                expected_code, actual
            ),
        }),
        Some(_) => None,
        None => match spec.exit_code_policy {
            ExitCodePolicy::DefaultZeroIfMissing => {
                let implicit = 0;
                if implicit != expected_code {
                    Some(VerifyOutcome::Fail {
                        reason: format!(
                            "exit_code_mismatch: expected {}, got {}",
                            expected_code, implicit
                        ),
                    })
                } else {
                    None
                }
            }
            ExitCodePolicy::LenientIfUnparsed => None,
        },
    }
}

fn verify_streams_contains(
    spec: &AcceptanceSpec,
    ev: &AcceptanceEvidence<'_>,
) -> Option<VerifyOutcome> {
    let tool_body =
        extract_json_output(ev.tool_output).unwrap_or_else(|| ev.tool_output.to_string());
    if let Some(ref expect_str) = spec.expect_stdout_contains {
        let haystack = haystack_for_stream_contains(ev, ev.stdout, tool_body.as_str());
        if !haystack.contains(expect_str) {
            return Some(VerifyOutcome::Fail {
                reason: format!("stdout_missing: expected to contain '{}'", expect_str),
            });
        }
    }

    if let Some(ref expect_str) = spec.expect_stderr_contains {
        let haystack = haystack_for_stream_contains(ev, ev.stderr, tool_body.as_str());
        if !haystack.contains(expect_str) {
            return Some(VerifyOutcome::Fail {
                reason: format!("stderr_missing: expected to contain '{}'", expect_str),
            });
        }
    }
    None
}

fn verify_combined_output_contains(
    spec: &AcceptanceSpec,
    ev: &AcceptanceEvidence<'_>,
) -> Option<VerifyOutcome> {
    if spec.expect_combined_output_contains.is_empty() {
        return None;
    }
    let raw = combined_haystack(ev);
    let haystack = if spec.combined_match_case_insensitive {
        raw.to_lowercase()
    } else {
        raw
    };
    for expected in &spec.expect_combined_output_contains {
        let needle = if spec.combined_match_case_insensitive {
            expected.to_lowercase()
        } else {
            expected.clone()
        };
        if !haystack.contains(&needle) {
            return Some(VerifyOutcome::Fail {
                reason: format!(
                    "combined_output_missing: expected to contain '{}'",
                    expected
                ),
            });
        }
    }
    None
}

fn verify_expected_files(
    spec: &AcceptanceSpec,
    ev: &AcceptanceEvidence<'_>,
) -> Option<VerifyOutcome> {
    for file_path in &spec.expect_file_exists {
        if file_path.trim().is_empty() {
            continue;
        }
        if !resolve_file(ev.workspace_root, file_path, ev.file_resolve) {
            let reason = match ev.file_resolve {
                FileResolveKind::AbsolutizeRelative => {
                    match crabmate_tools::workspace::path::absolutize_relative_under_root(
                        ev.workspace_root,
                        file_path,
                    ) {
                        Ok(_) => format!("file_not_found: '{}'", file_path),
                        Err(_) => format!("file_path_invalid: '{}'", file_path),
                    }
                }
                FileResolveKind::WorkspaceJoin => format!("file_not_found: '{}'", file_path),
            };
            return Some(VerifyOutcome::Fail { reason });
        }
    }
    None
}

fn verify_json_path_equals(
    spec: &AcceptanceSpec,
    ev: &AcceptanceEvidence<'_>,
) -> Option<VerifyOutcome> {
    let json_rule = spec.expect_json_path_equals.as_ref()?;

    let path = &json_rule.path;
    let expected = &json_rule.value;

    let json_output =
        extract_json_output(ev.tool_output).unwrap_or_else(|| ev.tool_output.to_string());

    match resolve_json_path_value(&json_output, path) {
        Ok(actual) => {
            if &actual != expected {
                Some(VerifyOutcome::Fail {
                    reason: format!(
                        "json_path_mismatch: path '{}' expected {}, got {}",
                        path, expected, actual
                    ),
                })
            } else {
                None
            }
        }
        Err(e) => Some(VerifyOutcome::Fail {
            reason: format!("json_path_error: path '{}' — {}", path, e.user_reason()),
        }),
    }
}

fn verify_http_status_branch(
    spec: &AcceptanceSpec,
    ev: &AcceptanceEvidence<'_>,
) -> Option<VerifyOutcome> {
    let expected_status = spec.expect_http_status?;

    let tool_name_lower = ev.tool_name.to_lowercase();
    let allow_http_probe = tool_name_lower.contains("http")
        || tool_name_lower.contains("fetch")
        || ev.tool_name.is_empty();
    if !allow_http_probe {
        return None;
    }

    if let Some(actual_status) = extract_http_status(ev.tool_name, ev.tool_output) {
        if actual_status != expected_status {
            return Some(VerifyOutcome::Fail {
                reason: format!(
                    "http_status_mismatch: expected {}, got {}",
                    expected_status, actual_status
                ),
            });
        }
        return None;
    }

    let exit_code = ev.tool_error.and_then(|e| e.code.parse::<i32>().ok());
    if exit_code == Some(0) && (200..300).contains(&expected_status) {
        return None;
    }
    if exit_code.is_none() {
        return Some(VerifyOutcome::Fail {
            reason: format!(
                "http_status_error: could not extract HTTP status code from output (expected {})",
                expected_status
            ),
        });
    }
    None
}

/// 对给定证据运行全部启用的验收项。
pub fn verify_against_spec(spec: &AcceptanceSpec, ev: &AcceptanceEvidence<'_>) -> VerifyOutcome {
    if spec.is_empty() {
        return VerifyOutcome::Pass;
    }

    if let Some(o) = verify_exit_code_branch(spec, ev) {
        return o;
    }
    if let Some(o) = verify_streams_contains(spec, ev) {
        return o;
    }
    if let Some(o) = verify_combined_output_contains(spec, ev) {
        return o;
    }
    if let Some(o) = verify_expected_files(spec, ev) {
        return o;
    }
    if let Some(o) = verify_json_path_equals(spec, ev) {
        return o;
    }
    if let Some(o) = verify_http_status_branch(spec, ev) {
        return o;
    }

    VerifyOutcome::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_default_zero_when_no_tool_error() {
        let spec = AcceptanceSpec {
            expect_exit_code: Some(0),
            exit_code_policy: ExitCodePolicy::DefaultZeroIfMissing,
            ..Default::default()
        };
        let ev = AcceptanceEvidence {
            tool_name: "x",
            tool_output: "",
            stdout: "",
            stderr: "",
            tool_error: None,
            fallback_exit_code: None,
            workspace_root: std::path::Path::new("/tmp"),
            file_resolve: FileResolveKind::AbsolutizeRelative,
            combined_text_override: None,
        };
        assert!(verify_against_spec(&spec, &ev).is_pass());
    }

    #[test]
    fn lenient_exit_skips_when_unparsed() {
        let spec = AcceptanceSpec {
            expect_exit_code: Some(0),
            exit_code_policy: ExitCodePolicy::LenientIfUnparsed,
            ..Default::default()
        };
        let ev = AcceptanceEvidence {
            tool_name: "x",
            tool_output: "no exit marker",
            stdout: "",
            stderr: "",
            tool_error: None,
            fallback_exit_code: None,
            workspace_root: std::path::Path::new("/tmp"),
            file_resolve: FileResolveKind::WorkspaceJoin,
            combined_text_override: None,
        };
        assert!(verify_against_spec(&spec, &ev).is_pass());
    }

    #[test]
    fn archive_unpack_stdout_contains_matches_tool_output_body() {
        let spec = AcceptanceSpec {
            expect_stdout_contains: Some("已解压".to_string()),
            ..Default::default()
        };
        let ev = AcceptanceEvidence {
            tool_name: "archive_unpack",
            tool_output: "已解压 187 个文件到: .",
            stdout: "",
            stderr: "",
            tool_error: None,
            fallback_exit_code: None,
            workspace_root: std::path::Path::new("/tmp"),
            file_resolve: FileResolveKind::AbsolutizeRelative,
            combined_text_override: None,
        };
        assert!(verify_against_spec(&spec, &ev).is_pass());
    }

    #[test]
    fn combined_insensitive_requires_all_substrings() {
        let spec = AcceptanceSpec {
            expect_combined_output_contains: vec!["hello".to_string(), "WORLD".to_string()],
            combined_match_case_insensitive: true,
            ..Default::default()
        };
        let ev = AcceptanceEvidence {
            tool_name: "x",
            tool_output: "",
            stdout: "",
            stderr: "",
            tool_error: None,
            fallback_exit_code: None,
            workspace_root: std::path::Path::new("/tmp"),
            file_resolve: FileResolveKind::WorkspaceJoin,
            combined_text_override: Some("Hello, world!"),
        };
        assert!(verify_against_spec(&spec, &ev).is_pass());
    }
}
