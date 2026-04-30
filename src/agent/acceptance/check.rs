//! 对 [`super::AcceptanceSpec`] 与 [`super::AcceptanceEvidence`] 执行逐项断言。

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

/// 尝试从输出中解析 HTTP 状态码（用于 http_request/http_fetch 类工具）
fn extract_http_status(tool_name: &str, output: &str) -> Option<u16> {
    let normalized = extract_json_output(output).unwrap_or_else(|| output.to_string());

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&normalized) {
        for field in ["status", "status_code", "http_status", "httpStatusCode"] {
            if let Some(val) = v.get(field)
                && let Some(code) = val.as_u64()
            {
                return Some(code as u16);
            }
        }
    }

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

    if tool_name.contains("http") && (output.contains("退出码：0") || output.contains("(exit=0)"))
    {
        return Some(200);
    }

    None
}

fn get_json_path_value(json_str: &str, path: &str) -> Option<serde_json::Value> {
    let value: serde_json::Value = serde_json::from_str(json_str).ok()?;

    let mut current = &value;
    let parts: Vec<&str> = path.split('.').collect();

    for (i, part) in parts.iter().enumerate() {
        let part = *part;

        if part.starts_with('$') && i == 0 {
            let rest = &part[1..];
            if rest.is_empty() {
                continue;
            }
            if let Some((idx, next)) = rest.split_once('[') {
                if !idx.is_empty() {
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

        if let Some((field, rest)) = part.split_once('[') {
            if !field.is_empty() {
                current = current.get(field)?;
            }
            let idx_str = rest.trim_start_matches('[').trim_end_matches(']');
            if let Ok(index) = idx_str.parse::<usize>() {
                current = current.get(index)?;
            }
            if let Some((_, next_field)) = rest.split_once(']').and_then(|(_, b)| b.split_once('.'))
                && !next_field.is_empty()
            {
                current = current.get(next_field)?;
            }
            continue;
        }

        if !part.is_empty() {
            current = current.get(part)?;
        }
    }

    Some(current.clone())
}

fn resolve_file(workspace_root: &std::path::Path, path: &str, kind: FileResolveKind) -> bool {
    match kind {
        FileResolveKind::AbsolutizeRelative => {
            match crate::workspace::path::absolutize_relative_under_root(workspace_root, path) {
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

/// 对给定证据运行全部启用的验收项。
pub fn verify_against_spec(spec: &AcceptanceSpec, ev: &AcceptanceEvidence<'_>) -> VerifyOutcome {
    if spec.is_empty() {
        return VerifyOutcome::Pass;
    }

    if let Some(expected_code) = spec.expect_exit_code {
        let parsed_structured = ev
            .tool_error
            .and_then(|e| e.code.parse::<i32>().ok())
            .or(ev.fallback_exit_code);

        match parsed_structured {
            Some(actual) if actual != expected_code => {
                return VerifyOutcome::Fail {
                    reason: format!(
                        "exit_code_mismatch: expected {}, got {}",
                        expected_code, actual
                    ),
                };
            }
            Some(_) => {}
            None => match spec.exit_code_policy {
                ExitCodePolicy::DefaultZeroIfMissing => {
                    let implicit = 0;
                    if implicit != expected_code {
                        return VerifyOutcome::Fail {
                            reason: format!(
                                "exit_code_mismatch: expected {}, got {}",
                                expected_code, implicit
                            ),
                        };
                    }
                }
                ExitCodePolicy::LenientIfUnparsed => {
                    // 与 GoalVerifier：无法解析则不因退出码拒绝
                }
            },
        }
    }

    if let Some(ref expect_str) = spec.expect_stdout_contains
        && !ev.stdout.contains(expect_str)
    {
        return VerifyOutcome::Fail {
            reason: format!("stdout_missing: expected to contain '{}'", expect_str),
        };
    }

    if let Some(ref expect_str) = spec.expect_stderr_contains
        && !ev.stderr.contains(expect_str)
    {
        return VerifyOutcome::Fail {
            reason: format!("stderr_missing: expected to contain '{}'", expect_str),
        };
    }

    if !spec.expect_combined_output_contains.is_empty() {
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
                return VerifyOutcome::Fail {
                    reason: format!("输出不包含期望内容: '{}'", expected),
                };
            }
        }
    }

    for file_path in &spec.expect_file_exists {
        if file_path.trim().is_empty() {
            continue;
        }
        if !resolve_file(ev.workspace_root, file_path, ev.file_resolve) {
            let reason = match ev.file_resolve {
                FileResolveKind::AbsolutizeRelative => {
                    match crate::workspace::path::absolutize_relative_under_root(
                        ev.workspace_root,
                        file_path,
                    ) {
                        Ok(_) => format!("file_not_found: '{}'", file_path),
                        Err(_) => format!("file_path_invalid: '{}'", file_path),
                    }
                }
                FileResolveKind::WorkspaceJoin => format!("期望文件不存在: {}", file_path),
            };
            return VerifyOutcome::Fail { reason };
        }
    }

    if let Some(json_rule) = &spec.expect_json_path_equals {
        let path = &json_rule.path;
        let expected = &json_rule.value;

        let json_output =
            extract_json_output(ev.tool_output).unwrap_or_else(|| ev.tool_output.to_string());

        match get_json_path_value(&json_output, path) {
            Some(actual) => {
                if &actual != expected {
                    return VerifyOutcome::Fail {
                        reason: format!(
                            "json_path_mismatch: path '{}' expected {}, got {}",
                            path, expected, actual
                        ),
                    };
                }
            }
            None => {
                return VerifyOutcome::Fail {
                    reason: format!(
                        "json_path_error: could not extract value at path '{}'",
                        path
                    ),
                };
            }
        }
    }

    if let Some(expected_status) = spec.expect_http_status {
        let tool_name_lower = ev.tool_name.to_lowercase();
        let allow_http_probe = tool_name_lower.contains("http")
            || tool_name_lower.contains("fetch")
            || ev.tool_name.is_empty();
        if allow_http_probe {
            if let Some(actual_status) = extract_http_status(ev.tool_name, ev.tool_output) {
                if actual_status != expected_status {
                    return VerifyOutcome::Fail {
                        reason: format!(
                            "http_status_mismatch: expected {}, got {}",
                            expected_status, actual_status
                        ),
                    };
                }
            } else {
                let exit_code = ev.tool_error.and_then(|e| e.code.parse::<i32>().ok());
                if exit_code == Some(0) && (200..300).contains(&expected_status) {
                    // exit 0 对于 HTTP 工具通常表示成功
                } else if exit_code.is_none() {
                    return VerifyOutcome::Fail {
                        reason: format!(
                            "http_status_error: could not extract HTTP status code from output (expected {})",
                            expected_status
                        ),
                    };
                }
            }
        }
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
