//! 统一工具执行结果：用于工作流等编排场景的结构化状态判断。
//!
//! ## 写入对话历史的 `role: tool` 信封（可选，见配置项 **`tool_result_envelope_v1`** / **`AGENT_TOOL_RESULT_ENVELOPE_V1`**）
//!
//! 顶层键 **`crabmate_tool`**，内含 `v`（**载荷版本**，当前为 **1**；与 SSE 整条控制面的 **`SseMessage.v`** 不同）、`name`、`summary`（与 SSE / `summarize_tool_call` 同源）、
//! `ok`、`exit_code`、`error_code`、`output`（工具原始返回正文，供模型阅读或再解析）。
//! SSE `tool_result` 对象另含 **`result_version`**，与 `crabmate_tool.v` 对齐，便于客户端区分「控制面版本」与「工具结果载荷版本」。
//! 可选扩展（见 [`ToolEnvelopeContext`]）：**`tool_call_id`**、**`execution_mode`**（`serial` / `parallel_readonly_batch`）、
//! **`parallel_batch_id`**（同批并行只读工具共享）、失败时的 **`failure_category`**（与 [`tool_error::ToolFailureCategory`] 蛇形字符串同源，由 **`error_code`** 推导）、**`retryable`**（与 `error_code` 配套的启发式，非保证）。
//! 经 [`maybe_compress_tool_message_content`] 截断时，会保留 **`output` 的首尾采样**（便于 grep/构建日志等仍见上下文），并写入
//! **`output_truncated`**、**`output_original_chars`**、**`output_kept_head_chars`**、**`output_kept_tail_chars`** 供模型与 UI 引用。
//!
//! 读路径请优先经 [`normalize::NormalizedToolEnvelope`]（[`normalize_tool_message_content`]），避免在展示层重复解析 `crabmate_tool` 字段。

mod normalize;
mod tool_error;

pub use normalize::{
    CRABMATE_TOOL_ENVELOPE_VERSION_V1, NormalizedToolEnvelope, normalize_tool_message_content,
};
#[allow(unused_imports)] // `pub use` 再导出供外部使用，本文件不直接引用
pub use tool_error::{ToolError, ToolFailureCategory, failure_category_for_error_code};

use std::borrow::Cow;

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
        Self::from_parsed(output, parsed)
    }

    /// 已由 [`parse_legacy_output`] 解析过的输出（与 `tools::run_tool_result` 单次解析路径共用）。
    pub(crate) fn from_parsed(output: String, parsed: ParsedLegacyOutput) -> Self {
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

/// 与 `error_code` 配套的**启发式**：是否值得由编排层自动重试（超时、工作流汇合类）；多数业务失败为 `false`。
/// 前端/模型仅作提示，**不**替代各工具的真实语义。
pub fn tool_error_retryable_heuristic(error_code: Option<&str>) -> bool {
    matches!(
        error_code,
        Some(
            "timeout"
                | "rate_limited"
                | "workflow_tool_join_error"
                | "workflow_semaphore_closed"
                | "workflow_node_missing_result"
        )
    )
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

/// 写入 `crabmate_tool` 时的可选关联字段（与 SSE `tool_result` 对齐）。
#[derive(Debug, Clone, Copy)]
pub struct ToolEnvelopeContext<'a> {
    pub tool_call_id: &'a str,
    /// `serial` 或 `parallel_readonly_batch`
    pub execution_mode: &'a str,
    /// 仅 `parallel_readonly_batch` 时有值；同批内多工具共享同一 id。
    pub parallel_batch_id: Option<&'a str>,
}

/// 将工具结果编码为单行 JSON，写入 `Message.content`（`role: tool`），便于下游按字段聚合/统计。
/// `summary` 须与 SSE `ToolResultBody.summary` 及 `summarize_tool_call*` 一致。
/// `envelope_ctx` 为 `None` 时不写入关联字段（兼容旧测试与外部回放数据）。
pub fn encode_tool_message_envelope_v1(
    tool_name: &str,
    summary: String,
    parsed: &ParsedLegacyOutput,
    raw_output: &str,
    envelope_ctx: Option<&ToolEnvelopeContext<'_>>,
) -> String {
    NormalizedToolEnvelope::from_tool_run(tool_name, summary, parsed, raw_output, envelope_ctx)
        .encode_to_message_line()
}

/// 从 `role: tool` 正文中取出用于 **JSON 再解析** 的载荷（如 `workflow_validate_result`）。
/// 非信封或解析失败时返回 trim 后的 `content` 借用。
pub fn tool_message_payload_for_inner_parse<'a>(content: &'a str) -> Cow<'a, str> {
    if let Some(env) = normalize_tool_message_content(content) {
        return Cow::Owned(env.output);
    }
    Cow::Borrowed(content.trim())
}

/// 从已写入对话历史的 `role: tool` `content` 判断工具是否**成功**（与信封 `ok` 或 `parse_legacy_output` 一致）。
/// `tool_name_fallback` 在非信封正文时用于 `parse_legacy_output` 的错误码归类。
pub fn tool_message_content_ok_for_model(content: &str, tool_name_fallback: &str) -> bool {
    if let Some(env) = normalize_tool_message_content(content) {
        return env.ok;
    }
    parse_legacy_output(tool_name_fallback, content.trim()).ok
}

/// 为 `output` 字段生成首尾采样正文（Unicode 标量计数），`max_output_chars` 为**整个**替换后 `output` 字符串的字符上限。
fn tool_output_head_tail_sample(original: &str, max_output_chars: usize) -> (String, usize, usize) {
    let total = original.chars().count();
    debug_assert!(total > max_output_chars);
    // 分隔说明与尾注占用预算，避免采样后仍超 `tool_message_max_chars` 触发反复压缩
    const MARKER_OVERHEAD: usize = 160;
    let inner_budget = max_output_chars.saturating_sub(MARKER_OVERHEAD).max(16);
    let half = inner_budget / 2;
    let head_n = half.max(1).min(inner_budget.saturating_sub(1));
    let tail_n = inner_budget.saturating_sub(head_n).max(1);
    let head: String = original.chars().take(head_n).collect();
    let tail: String = original
        .chars()
        .rev()
        .take(tail_n)
        .collect::<Vec<char>>()
        .into_iter()
        .rev()
        .collect();
    let omitted = total.saturating_sub(head_n + tail_n);
    let body = format!(
        "{head}\n\n---\n…（省略约 {omitted} 字符）…\n---\n\n{tail}\n\n\
         [输出已采样：原文约 {total} 字符；仅首尾片段进入模型上下文，可按路径缩小范围或分页读取后重试。]",
    );
    (body, head_n, tail_n)
}

/// 对过长 `role: tool` 正文截断：若为 [`encode_tool_message_envelope_v1`] 形状，对 **`output`** 做**首尾采样**并写入
/// `output_truncated` / `output_original_chars` / `output_kept_*`；否则整段按前缀截断。
pub fn maybe_compress_tool_message_content(content: &str, max_chars: usize) -> Option<String> {
    let max_chars = max_chars.max(256);
    let total_chars = content.chars().count();
    if total_chars <= max_chars {
        return None;
    }
    if let Some(mut env) = normalize_tool_message_content(content) {
        let out_chars = env.output.chars().count();
        if out_chars > max_chars {
            let (sampled, head_n, tail_n) = tool_output_head_tail_sample(&env.output, max_chars);
            env.output = sampled;
            env.output_truncated = true;
            env.output_original_chars = Some(out_chars as u64);
            env.output_kept_head_chars = Some(head_n as u64);
            env.output_kept_tail_chars = Some(tail_n as u64);
            return Some(env.encode_to_message_line());
        }
        return None;
    }
    let truncated: String = content.chars().take(max_chars).collect();
    Some(format!(
        "{}\n\n[... 已截断，原始约 {} 字符，保留前 {} 字符 ...]",
        truncated, total_chars, max_chars
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

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
        let env = encode_tool_message_envelope_v1("run_command", "s".into(), &parsed, raw, None);
        assert!(!tool_message_content_ok_for_model(&env, "run_command"));
        let ok_raw = "退出码：0\n标准输出：\nhi\n";
        let ok_parsed = parse_legacy_output("run_command", ok_raw);
        let ok_env =
            encode_tool_message_envelope_v1("run_command", "s".into(), &ok_parsed, ok_raw, None);
        assert!(tool_message_content_ok_for_model(&ok_env, "run_command"));
    }

    #[test]
    fn envelope_roundtrip_and_inner_payload() {
        let raw = "退出码：0\n标准输出：\nhi\n";
        let parsed = parse_legacy_output("run_command", raw);
        let s = encode_tool_message_envelope_v1("run_command", "true".into(), &parsed, raw, None);
        assert!(s.contains("crabmate_tool"));
        assert!(s.contains("\"summary\":\"true\""));
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
        let env = encode_tool_message_envelope_v1("x", "s".into(), &parsed, &long, None);
        let out = maybe_compress_tool_message_content(&env, 100).expect("compress");
        assert!(out.len() < env.len());
        let inner = tool_message_payload_for_inner_parse(&out);
        assert!(
            inner.contains("输出已采样") || inner.contains("省略约"),
            "expected head/tail sample markers in {}",
            inner
        );
        let v: Value = serde_json::from_str(&out).expect("json");
        let ct = v
            .get("crabmate_tool")
            .and_then(|x| x.as_object())
            .expect("ct");
        assert_eq!(
            ct.get("output_truncated").and_then(|x| x.as_bool()),
            Some(true)
        );
        assert_eq!(
            ct.get("output_original_chars").and_then(|x| x.as_u64()),
            Some(500)
        );
    }

    #[test]
    fn envelope_includes_retryable_on_failure() {
        let raw = "错误：超时\n";
        let parsed = parse_legacy_output("run_command", raw);
        let s = encode_tool_message_envelope_v1("run_command", "s".into(), &parsed, raw, None);
        let v: Value = serde_json::from_str(&s).unwrap();
        let ct = v.get("crabmate_tool").unwrap();
        assert_eq!(ct.get("retryable").and_then(|x| x.as_bool()), Some(true));
        assert_eq!(
            ct.get("failure_category").and_then(|x| x.as_str()),
            Some("timeout")
        );
    }

    #[test]
    fn envelope_includes_tool_call_id_and_batch() {
        let raw = "退出码：0\n";
        let parsed = parse_legacy_output("read_file", raw);
        let ctx = ToolEnvelopeContext {
            tool_call_id: "call_abc",
            execution_mode: "parallel_readonly_batch",
            parallel_batch_id: Some("pb-1"),
        };
        let s = encode_tool_message_envelope_v1("read_file", "s".into(), &parsed, raw, Some(&ctx));
        let v: Value = serde_json::from_str(&s).unwrap();
        let ct = v.get("crabmate_tool").unwrap();
        assert_eq!(
            ct.get("tool_call_id").and_then(|x| x.as_str()),
            Some("call_abc")
        );
        assert_eq!(
            ct.get("execution_mode").and_then(|x| x.as_str()),
            Some("parallel_readonly_batch")
        );
        assert_eq!(
            ct.get("parallel_batch_id").and_then(|x| x.as_str()),
            Some("pb-1")
        );
    }
}

#[cfg(test)]
mod golden_envelope_tests {
    use std::fs;
    use std::path::PathBuf;

    use serde_json::Value;

    use super::normalize_tool_message_content;

    #[test]
    fn tool_result_envelope_golden_roundtrip() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("fixtures/tool_result_envelope_golden.jsonl");
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let mut parts = t.splitn(2, '\t');
            let label = parts.next().unwrap_or("?");
            let expected_line = parts
                .next()
                .unwrap_or_else(|| panic!("line {}: missing tab-separated JSON", line_no + 1));
            let expected: Value = serde_json::from_str(expected_line).unwrap_or_else(|e| {
                panic!(
                    "line {} ({}): invalid expected JSON: {e}",
                    line_no + 1,
                    label
                )
            });
            let content = expected_line.to_string();
            let norm = normalize_tool_message_content(&content).unwrap_or_else(|| {
                panic!(
                    "line {} ({}): normalize_tool_message_content returned None",
                    line_no + 1,
                    label
                )
            });
            let round = norm.encode_to_message_line();
            let got: Value = serde_json::from_str(&round).unwrap_or_else(|e| {
                panic!(
                    "line {} ({}): round-trip JSON invalid: {e}",
                    line_no + 1,
                    label
                )
            });
            assert_eq!(
                got,
                expected,
                "line {} ({}): round-trip mismatch\nexpected: {}\n     got: {}",
                line_no + 1,
                label,
                expected_line,
                round
            );
        }
    }
}
