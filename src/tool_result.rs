//! 统一工具执行结果：用于工作流等编排场景的结构化状态判断。
//!
//! ## 写入对话历史的 `role: tool` 信封（可选，见配置项 **`tool_result_envelope_v1`** / **`AGENT_TOOL_RESULT_ENVELOPE_V1`**）
//!
//! 顶层键 **`crabmate_tool`**，内含 `v`（当前为 **1**）、`name`、`summary`（与 SSE / `summarize_tool_call` 同源）、
//! `ok`、`exit_code`、`error_code`、`output`（工具原始返回正文，供模型阅读或再解析）。

use std::borrow::Cow;

use serde_json::{Map, Value};

#[derive(Debug, Clone)]
pub struct ToolResult {
    /// 工具调用是否成功（由退出码或错误语义推断）
    pub ok: bool,
    /// 若输出可解析出退出码，则填充该字段
    pub exit_code: Option<i32>,
    /// 原始输出（兼容现有前端/模型消费逻辑）
    pub message: String,
    /// 若可抽取，标准输出文本
    pub stdout: String,
    /// 若可抽取，标准错误文本
    pub stderr: String,
    /// 机器可读错误码（失败时填充）
    pub error_code: Option<String>,
}

/// 兼容旧字符串输出的解析结果（不复制整段 `message`）。
#[derive(Debug, Clone)]
pub struct ParsedLegacyOutput {
    pub ok: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub error_code: Option<String>,
}

/// 解析旧格式工具输出，仅返回状态与分流字段，避免复制完整 message。
pub fn parse_legacy_output(tool_name: &str, output: &str) -> ParsedLegacyOutput {
    let first = output.lines().next().unwrap_or("").trim();
    let exit_code = parse_exit_code(first);
    let (stdout, stderr) = extract_streams(output);

    let ok = if let Some(code) = exit_code {
        code == 0
    } else {
        !looks_like_failure(first)
    };
    let error_code = if ok {
        None
    } else {
        Some(classify_error_code(first, tool_name))
    };

    ParsedLegacyOutput {
        ok,
        exit_code,
        stdout,
        stderr,
        error_code,
    }
}

impl ToolResult {
    /// 将既有“字符串工具输出”转换为结构化结果。
    pub fn from_legacy_output(tool_name: &str, output: String) -> Self {
        let parsed = parse_legacy_output(tool_name, &output);

        Self {
            ok: parsed.ok,
            exit_code: parsed.exit_code,
            message: output,
            stdout: parsed.stdout,
            stderr: parsed.stderr,
            error_code: parsed.error_code,
        }
    }
}

fn parse_exit_code(first_line: &str) -> Option<i32> {
    if let Some(rest) = first_line.strip_prefix("退出码：") {
        return rest.trim().parse::<i32>().ok();
    }
    let idx = first_line.find("(exit=")?;
    let rest = &first_line[idx + "(exit=".len()..];
    let end = rest.find(')')?;
    rest[..end].trim().parse::<i32>().ok()
}

fn looks_like_failure(first_line: &str) -> bool {
    if first_line.is_empty() {
        return false;
    }
    first_line.starts_with("错误")
        || first_line.starts_with("未知工具")
        || first_line.starts_with("参数解析错误")
        || first_line.starts_with("执行失败")
        || first_line.contains("失败")
        || first_line.contains("超时")
}

fn classify_error_code(first_line: &str, tool_name: &str) -> String {
    if first_line.contains("参数解析错误") {
        return "invalid_args".to_string();
    }
    if first_line.contains("不允许的命令") {
        return "command_not_allowed".to_string();
    }
    if first_line.contains("未设置工作区") {
        return "workspace_not_set".to_string();
    }
    if first_line.contains("超时") {
        return "timeout".to_string();
    }
    if first_line.starts_with("未知工具") {
        return "unknown_tool".to_string();
    }
    format!("{}_failed", tool_name)
}

fn extract_streams(output: &str) -> (String, String) {
    let stdout_marker = "标准输出：\n";
    let stderr_marker = "标准错误：\n";

    let stdout = if let Some(pos) = output.find(stdout_marker) {
        let start = pos + stdout_marker.len();
        let end = output[start..]
            .find(stderr_marker)
            .map(|i| start + i)
            .unwrap_or(output.len());
        output[start..end].trim().to_string()
    } else {
        String::new()
    };
    let stderr = if let Some(pos) = output.find(stderr_marker) {
        let start = pos + stderr_marker.len();
        output[start..].trim().to_string()
    } else {
        String::new()
    };
    (stdout, stderr)
}

/// 将工具结果编码为单行 JSON，写入 `Message.content`（`role: tool`），便于下游按字段聚合/统计。
/// `summary` 须与 SSE `ToolResultBody.summary` 及 `summarize_tool_call*` 一致。
pub fn encode_tool_message_envelope_v1(
    tool_name: &str,
    summary: String,
    parsed: &ParsedLegacyOutput,
    raw_output: &str,
) -> String {
    let mut ct = Map::new();
    ct.insert("v".into(), Value::from(1_u32));
    ct.insert("name".into(), Value::String(tool_name.to_string()));
    ct.insert("summary".into(), Value::String(summary));
    ct.insert("ok".into(), Value::Bool(parsed.ok));
    ct.insert("output".into(), Value::String(raw_output.to_string()));
    if let Some(c) = parsed.exit_code {
        ct.insert("exit_code".into(), Value::from(c));
    }
    if let Some(ref e) = parsed.error_code {
        ct.insert("error_code".into(), Value::String(e.clone()));
    }
    let mut root = Map::new();
    root.insert("crabmate_tool".into(), Value::Object(ct));
    serde_json::to_string(&Value::Object(root)).unwrap_or_else(|_| raw_output.to_string())
}

/// 从 `role: tool` 正文中取出用于 **JSON 再解析** 的载荷（如 `workflow_validate_result`）。
/// 非信封或解析失败时返回 trim 后的 `content` 借用。
/// 从已写入对话历史的 `role: tool` `content` 判断工具是否**成功**（与信封 `ok` 或 `parse_legacy_output` 一致）。
/// `tool_name_fallback` 在非信封正文时用于 `parse_legacy_output` 的错误码归类。
pub fn tool_message_content_ok_for_model(content: &str, tool_name_fallback: &str) -> bool {
    let trimmed = content.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed)
        && let Some(ct) = v.get("crabmate_tool").and_then(|x| x.as_object())
    {
        if let Some(ok) = ct.get("ok").and_then(|x| x.as_bool()) {
            return ok;
        }
        let name = ct
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or(tool_name_fallback);
        let output = ct.get("output").and_then(|x| x.as_str()).unwrap_or("");
        return parse_legacy_output(name, output).ok;
    }
    parse_legacy_output(tool_name_fallback, trimmed).ok
}

pub fn tool_message_payload_for_inner_parse<'a>(content: &'a str) -> Cow<'a, str> {
    let t = content.trim();
    let Ok(v) = serde_json::from_str::<Value>(t) else {
        return Cow::Borrowed(t);
    };
    let Some(ct) = v.get("crabmate_tool") else {
        return Cow::Borrowed(t);
    };
    let Some(Value::String(out)) = ct.get("output") else {
        return Cow::Borrowed(t);
    };
    Cow::Owned(out.clone())
}

/// 对过长 `role: tool` 正文截断：若为 [`encode_tool_message_envelope_v1`] 形状，只截断 `output` 并保留信封元数据。
pub fn maybe_compress_tool_message_content(content: &str, max_chars: usize) -> Option<String> {
    let max_chars = max_chars.max(256);
    let total_chars = content.chars().count();
    if total_chars <= max_chars {
        return None;
    }
    if let Ok(mut v) = serde_json::from_str::<Value>(content)
        && let Some(Value::Object(ct)) = v.get_mut("crabmate_tool")
        && let Some(Value::String(out)) = ct.get_mut("output")
    {
        let out_chars = out.chars().count();
        if out_chars > max_chars {
            let truncated: String = out.chars().take(max_chars).collect();
            *out = format!(
                "{}\n\n[... 已截断，原始约 {} 字符，保留前 {} 字符 ...]",
                truncated, out_chars, max_chars
            );
            return serde_json::to_string(&v).ok();
        }
    }
    let truncated: String = content.chars().take(max_chars).collect();
    Some(format!(
        "{}\n\n[... 已截断，原始约 {} 字符，保留前 {} 字符 ...]",
        truncated, total_chars, max_chars
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exit_code_from_chinese_prefix() {
        let r = ToolResult::from_legacy_output(
            "run_command",
            "退出码：0\n标准输出：\nhello\n".to_string(),
        );
        assert!(r.ok);
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.stdout, "hello");
    }

    #[test]
    fn parse_exit_code_from_exit_pattern() {
        let r = ToolResult::from_legacy_output(
            "cargo_test",
            "cargo test (exit=1):\nfailed".to_string(),
        );
        assert!(!r.ok);
        assert_eq!(r.exit_code, Some(1));
        assert_eq!(r.error_code.as_deref(), Some("cargo_test_failed"));
    }

    #[test]
    fn classify_workspace_error_without_exit_code() {
        let r = ToolResult::from_legacy_output("run_command", "错误：未设置工作区".to_string());
        assert!(!r.ok);
        assert_eq!(r.exit_code, None);
        assert_eq!(r.error_code.as_deref(), Some("workspace_not_set"));
    }

    #[test]
    fn tool_message_content_ok_reads_envelope_ok() {
        let raw = "错误：不允许的命令\n";
        let parsed = parse_legacy_output("run_command", raw);
        let env = encode_tool_message_envelope_v1("run_command", "s".into(), &parsed, raw);
        assert!(!tool_message_content_ok_for_model(&env, "run_command"));
        let ok_raw = "退出码：0\n标准输出：\nhi\n";
        let ok_parsed = parse_legacy_output("run_command", ok_raw);
        let ok_env = encode_tool_message_envelope_v1("run_command", "s".into(), &ok_parsed, ok_raw);
        assert!(tool_message_content_ok_for_model(&ok_env, "run_command"));
    }

    #[test]
    fn envelope_roundtrip_and_inner_payload() {
        let raw = "退出码：0\n标准输出：\nhi\n";
        let parsed = parse_legacy_output("run_command", raw);
        let s =
            encode_tool_message_envelope_v1("run_command", "执行命令：true".into(), &parsed, raw);
        assert!(s.contains("crabmate_tool"));
        assert!(s.contains("\"summary\":\"执行命令：true\""));
        let inner = tool_message_payload_for_inner_parse(&s);
        assert_eq!(inner.as_ref(), raw);
    }

    #[test]
    fn inner_parse_passes_through_plain_and_legacy_json() {
        let j = r#"{"report_type":"workflow_validate_result","spec":{"layer_count":2}}"#;
        assert_eq!(tool_message_payload_for_inner_parse(j).as_ref(), j);
        assert_eq!(
            tool_message_payload_for_inner_parse(" plain ").as_ref(),
            "plain"
        );
    }

    #[test]
    fn compress_envelope_truncates_output_only() {
        let long = "x".repeat(500);
        let parsed = parse_legacy_output("x", &long);
        let env = encode_tool_message_envelope_v1("x", "s".into(), &parsed, &long);
        let out = maybe_compress_tool_message_content(&env, 100).expect("compress");
        assert!(out.len() < env.len());
        let inner = tool_message_payload_for_inner_parse(&out);
        assert!(inner.contains("[... 已截断"));
    }
}
