//! 统一工具执行结果：用于工作流等编排场景的结构化状态判断。
//!
//! ## 写入对话历史的 `role: tool` 信封（可选，见配置项 **`tool_result_envelope_v1`** / **`CM_TOOL_RESULT_ENVELOPE_V1`**）
//!
//! 顶层键 **`crabmate_tool`**，内含 `v`（**载荷版本**，当前为 **1**；与 SSE 整条控制面的 **`SseMessage.v`** 不同）、`name`、`summary`（与 SSE / `summarize_tool_call` 同源）、
//! `ok`、`exit_code`、`error_code`、`output`（工具原始返回正文，供模型阅读或再解析）。
//! 可选 **`structured_payload`**：机器可读的小对象；**`schema`** 见 **`docs/工具说明.md`**（**`run_command_exit_v1`**、**`subprocess_exit_v1`**、**`http_tool_response_v1`** 等），与正文并存；SSE **`tool_result.structured_preview`** 可与首行 **`crabmate_tool_output`** 合并下发。
//! SSE `tool_result` 对象另含 **`result_version`**，与 `crabmate_tool.v` 对齐，便于客户端区分「控制面版本」与「工具结果载荷版本」。
//! 可选扩展（见 [`ToolEnvelopeContext`]）：**`tool_call_id`**、**`execution_mode`**（`serial` / `parallel_readonly_batch`）、
//! **`parallel_batch_id`**（同批并行只读工具共享）、失败时的 **`failure_category`**（与 [`tool_error::ToolFailureCategory`] 蛇形字符串同源，由 **`error_code`** 推导）、**`retryable`**（与 `error_code` 配套的启发式，非保证）。
//! 经 [`maybe_compress_tool_message_content`] 截断时，会保留 **`output` 的首尾采样**（便于 grep/构建日志等仍见上下文），并写入
//! **`output_truncated`**、**`output_original_chars`**、**`output_kept_head_chars`**、**`output_kept_tail_chars`** 供模型与 UI 引用。
//!
//! 读路径请优先经 [`normalize::NormalizedToolEnvelope`]（[`normalize_tool_message_content`]），避免在展示层重复解析 `crabmate_tool` 字段。

mod normalize;
mod output_header;
mod tool_error;

pub use normalize::{
    CRABMATE_TOOL_ENVELOPE_VERSION_V1, NormalizedToolEnvelope, normalize_tool_message_content,
};
pub use output_header::{
    CRABMATE_TOOL_OUTPUT_KIND, CRABMATE_TOOL_OUTPUT_VERSION, CrabmateToolOutputEnvelope,
    CrabmateToolOutputMeta, ListTreeOutputFields, PREVIEW_WORKSPACE_WRITE_DIFF,
    ReadDirOutputFields, ReadFileOutputFields, SearchInFilesOutputFields, WorkspaceWriteDiffFields,
    WorkspaceWriteDiffFile, prepend_crabmate_tool_output,
};
#[allow(unused_imports)] // `pub use` 再导出供外部使用，本文件不直接引用
pub use tool_error::{ToolError, ToolFailureCategory, failure_category_for_error_code};

use std::borrow::Cow;

use serde_json::Value;

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

/// 跳过 `run_command` 正文可能开头的 `命令：…` 行，使后续仍以 `退出码：` 行为准。
fn strip_leading_command_invocation_line(output: &str) -> &str {
    let mut s = output.trim_start();
    if let Some(idx) = s.find('\n') {
        let first = s[..idx].trim();
        if first.starts_with("命令：") {
            s = s[idx + 1..].trim_start();
        }
    }
    s
}

/// 首行是否为 `crabmate_tool_output` 结构化头。
fn is_tool_output_header(first: &str) -> bool {
    CrabmateToolOutputMeta::parse_line(first).is_some()
        || first.contains("\"kind\":\"crabmate_tool_output\"") // 非法 JSON 回退；合法头走 parse_line
}

fn body_after_header_looks_like_failure(body: &str) -> bool {
    body.lines().skip(1).any(|l| looks_like_failure(l.trim()))
}

/// 按 **header 契约** 判断，不维护工具名白名单：
/// 1. 头上显式 `ok` → 以它为准（生产端已知成败）；
/// 2. `preview=workspace_write_diff` → 写预览可与失败正文并存，扫 header **之后**的状态行；
/// 3. 其余（`read_file` 等载荷头）错误路径不带该头，有头即成功，避免扫文件/命中正文。
fn structured_header_output_ok(header: &CrabmateToolOutputMeta, body: &str) -> bool {
    if let Some(ok) = header.ok {
        return ok;
    }
    if header.is_workspace_write_diff() {
        return !body_after_header_looks_like_failure(body);
    }
    true
}

/// 解析旧格式工具输出，仅返回状态与分流字段，避免复制完整 message。
pub fn parse_legacy_output(tool_name: &str, output: &str) -> ParsedLegacyOutput {
    let body = strip_leading_command_invocation_line(output);
    let first = body.lines().next().unwrap_or("").trim();
    let exit_code = parse_exit_code(first);
    let (stdout, stderr) = extract_streams(output);

    let ok = if let Some(header) = CrabmateToolOutputMeta::parse_line(first) {
        structured_header_output_ok(&header, body)
    } else if is_tool_output_header(first) {
        !body_after_header_looks_like_failure(body)
    } else if let Some(code) = exit_code {
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
    pub fn from_parsed(output: String, parsed: ParsedLegacyOutput) -> Self {
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
    if first_line.contains("检测到同命令重复失败") {
        return "repeated_tool_failure_short_circuit".to_string();
    }
    if first_line.contains("检测到同类失败已发生") {
        return "repeated_tool_family_failure_short_circuit".to_string();
    }
    if first_line.contains("参数解析错误") {
        return "invalid_args".to_string();
    }
    if first_line.contains("参数与工具 JSON Schema") {
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

/// 供 **`crabmate_tool.structured_payload`** 与 SSE 合并路径使用：对已格式化命令输出生成稳定 JSON（**不含** stdout/stderr 全文）。
pub fn structured_payload_for_tool(tool_name: &str, raw_output: &str) -> Option<Value> {
    if tool_name == "run_command" {
        return Some(run_command_structured_payload(raw_output));
    }
    if matches!(tool_name, "http_fetch" | "http_request") {
        return http_tool_response_v1_payload(tool_name, raw_output);
    }
    if tool_name.starts_with("cargo_") || tool_name == "rust_rustc" {
        return subprocess_exit_v1_payload(tool_name, raw_output);
    }
    None
}

/// `output_util::format_exited_command_output` 形状：`{title} (exit=N):` + 正文。
fn subprocess_exit_v1_payload(tool_name: &str, raw_output: &str) -> Option<Value> {
    let first = raw_output.lines().next()?.trim();
    let (title, exit_code) = parse_title_exit_prefix_line(first)?;
    let ok = exit_code == 0;
    Some(serde_json::json!({
        "kind": "crabmate_structured_payload",
        "tool": tool_name,
        "version": 1_u64,
        "schema": "subprocess_exit_v1",
        "title": title,
        "exit_code": exit_code,
        "ok": ok,
    }))
}

fn parse_title_exit_prefix_line(line: &str) -> Option<(String, i32)> {
    let idx = line.rfind(" (exit=")?;
    let after = &line[idx + " (exit=".len()..];
    let end_paren = after.find(')')?;
    let code = after[..end_paren].trim().parse::<i32>().ok()?;
    let rest = after[end_paren + 1..].trim_start();
    if !rest.starts_with(':') {
        return None;
    }
    let title = line[..idx].trim().to_string();
    Some((title, code))
}

/// `http_fetch` / `http_request` 成功响应正文：`method:` / `请求 URL:` / `状态:` 等（见 **`http_fetch.rs`**）。
fn http_tool_response_v1_payload(tool_name: &str, raw_output: &str) -> Option<Value> {
    if raw_output.starts_with("错误：")
        || raw_output.contains("未匹配配置的 http_fetch_allowed_prefixes")
        || raw_output.starts_with("请求失败:")
        || raw_output.starts_with("读取响应体失败:")
        || raw_output.starts_with("HTTP 客户端构建失败")
        || raw_output.starts_with("json_body 序列化失败")
    {
        return None;
    }
    let mut method = None::<String>;
    let mut request_url = None::<String>;
    let mut status_line = None::<String>;
    for line in raw_output.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("method:") {
            method = Some(rest.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("请求 URL:") {
            request_url = Some(rest.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("状态:") {
            status_line = Some(rest.trim().to_string());
        }
    }
    let status_text = status_line?;
    let status_code = status_text
        .split_whitespace()
        .next()
        .and_then(|s| s.parse::<u16>().ok());
    let ok = status_code.is_some_and(|c| (200..300).contains(&c));
    let method = method?;
    let request_url = request_url?;
    Some(serde_json::json!({
        "kind": "crabmate_structured_payload",
        "tool": tool_name,
        "version": 1_u64,
        "schema": "http_tool_response_v1",
        "method": method,
        "request_url": request_url,
        "http_status": status_text,
        "http_status_code": status_code,
        "ok": ok,
    }))
}

fn run_command_structured_payload(raw_output: &str) -> Value {
    let invocation = raw_output
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("命令："))
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    let body = strip_leading_command_invocation_line(raw_output);
    let first = body.lines().next().unwrap_or("").trim();
    let exit_code = parse_exit_code(first);
    let ok = if let Some(code) = exit_code {
        code == 0
    } else {
        !looks_like_failure(first)
    };
    let (stdout, stderr) = extract_streams(raw_output);
    serde_json::json!({
        "kind": "crabmate_structured_payload",
        "tool": "run_command",
        "version": 1_u64,
        "schema": "run_command_exit_v1",
        "invocation": invocation,
        "ok": ok,
        "exit_code": exit_code,
        "stdout_nonempty": !stdout.is_empty(),
        "stderr_nonempty": !stderr.is_empty(),
    })
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
    let structured_payload = structured_payload_for_tool(tool_name, raw_output);
    NormalizedToolEnvelope::from_tool_run(
        tool_name,
        summary,
        parsed,
        raw_output,
        envelope_ctx,
        structured_payload,
    )
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
    fn parse_ok_when_first_line_is_tool_output_header_with_failure_word() {
        // diff 预览内容行含「失败」字样时，结构化头（成功路径生成）应判定成功而非 modify_file_failed。
        let raw = r##"{"files":[{"path":"conf.py","unified_diff":"--- a/conf.py\n+++ b/conf.py\n-构建失败重试说明\n+新内容\n","truncated":false}],"kind":"crabmate_tool_output","preview":"workspace_write_diff","preview_truncated":false,"tool":"modify_file","version":1}
路径：conf.py
已按行替换（行 4-15，共删除 12 行，写入新内容 763 字节）"##;
        let parsed = parse_legacy_output("modify_file", raw);
        assert!(parsed.ok);
        assert_eq!(parsed.error_code, None);
    }

    #[test]
    fn parse_ok_when_read_file_body_contains_failure_word() {
        // 文件短于 max_lines 时整段进入正文；内容含「失败」不得把成功读取标成 read_file_failed。
        let raw = r#"{"end_line_shown":3,"file_empty":false,"has_more":false,"kind":"crabmate_tool_output","line_count_returned":3,"path":"github_trending.py","start_line":1,"tool":"read_file","total_lines":null,"truncated_by_max_lines":false,"version":1}
文本编码: UTF-8
文件: github_trending.py
总行数: 未统计（大文件可避免 count_total_lines 以省时间）
本段行范围: 1-3（单次 max_lines=500）
已读到文件末尾（本段范围内无更多行）。

1|print("ok")
2|raise RuntimeError("请求失败")
3|# 超时重试"#;
        let parsed = parse_legacy_output("read_file", raw);
        assert!(parsed.ok, "short successful read must stay ok");
        assert_eq!(parsed.error_code, None);
    }

    #[test]
    fn parse_ok_when_search_hit_line_contains_failure_word() {
        let raw = r#"{"files_visited":1,"kind":"crabmate_tool_output","match_count":1,"max_results":200,"pattern":"Error","root":".","tool":"search_in_files","truncated":false,"version":1}
搜索："Error"
范围：.
a.py:10: raise RuntimeError("请求失败")"#;
        let parsed = parse_legacy_output("search_in_files", raw);
        assert!(parsed.ok);
        assert_eq!(parsed.error_code, None);
    }

    #[test]
    fn parse_ok_when_payload_header_even_if_tool_name_unknown() {
        // 不靠工具名白名单：无 write-diff preview 的载荷头，正文含「失败」仍为成功。
        let raw = r#"{"kind":"crabmate_tool_output","tool":"future_list","version":1}
条目：构建失败.log"#;
        let parsed = parse_legacy_output("future_list", raw);
        assert!(parsed.ok);
        assert_eq!(parsed.error_code, None);
    }

    #[test]
    fn parse_respects_explicit_ok_on_header() {
        let fail = r#"{"kind":"crabmate_tool_output","ok":false,"tool":"read_file","version":1}
文件: a.txt"#;
        let parsed = parse_legacy_output("read_file", fail);
        assert!(!parsed.ok);

        let ok_write = r##"{"kind":"crabmate_tool_output","ok":true,"preview":"workspace_write_diff","tool":"modify_file","version":1}
路径：a
错误：不应再扫正文"##;
        let parsed = parse_legacy_output("modify_file", ok_write);
        assert!(parsed.ok);
    }

    #[test]
    fn parse_fails_when_header_body_has_reject_error_line() {
        // full overwrite 未带 confirm 的拒绝路径：header 后正文含「错误：…未写盘」。
        let raw = r##"{"files":[{"path":"conf.py","unified_diff":"--- a/conf.py\n+++ b/conf.py\n-旧\n+新\n","truncated":false}],"kind":"crabmate_tool_output","preview":"workspace_write_diff","preview_truncated":false,"tool":"modify_file","version":1}
路径：conf.py
错误：本次整文件覆盖将大幅缩短、删去大量行或清空非空文件。**未写盘**。"##;
        let parsed = parse_legacy_output("modify_file", raw);
        assert!(!parsed.ok);
        assert_eq!(parsed.error_code.as_deref(), Some("modify_file_failed"));
    }

    #[test]
    fn parse_fails_when_header_body_has_patch_partial_failure() {
        // apply_patch 部分失败同样生成 header，正文带「失败：」行。
        let raw = r##"{"files":[{"path":"a.rs","unified_diff":"--- a/a.rs\n+++ b/a.rs\n+ok\n","truncated":false}],"kind":"crabmate_tool_output","preview":"workspace_write_diff","preview_truncated":false,"tool":"apply_patch","version":1}
补丁部分应用：
成功：
a.rs
失败：
b.rs"##;
        let parsed = parse_legacy_output("apply_patch", raw);
        assert!(!parsed.ok);
    }

    #[test]
    fn tool_output_header_rejects_plain_error_first_line() {
        assert!(!is_tool_output_header("错误：未设置工作区"));
        assert!(!is_tool_output_header("执行失败：xxx"));
    }

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
    fn parse_skips_command_invocation_prefix() {
        let raw = "命令：pwd\n退出码：0\n标准输出：\n/\n";
        let r = ToolResult::from_legacy_output("run_command", raw.to_string());
        assert!(r.ok);
        assert_eq!(r.exit_code, Some(0));
        assert_eq!(r.stdout, "/");
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
    fn classify_repeated_tool_short_circuit_errors() {
        let r = ToolResult::from_legacy_output(
            "run_command",
            "错误：检测到同命令重复失败，已短路本次调用（error=run_command_failed）。".to_string(),
        );
        assert!(!r.ok);
        assert_eq!(
            r.error_code.as_deref(),
            Some("repeated_tool_failure_short_circuit")
        );

        let r = ToolResult::from_legacy_output(
            "run_command",
            "错误：检测到同类失败已发生（family=cargo_manifest_missing），已短路本次调用。"
                .to_string(),
        );
        assert_eq!(
            r.error_code.as_deref(),
            Some("repeated_tool_family_failure_short_circuit")
        );
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
    fn envelope_includes_structured_payload_subprocess_cargo() {
        let raw = "cargo check (exit=0):\n(check output)";
        let parsed = parse_legacy_output("cargo_check", raw);
        let s = encode_tool_message_envelope_v1("cargo_check", "s".into(), &parsed, raw, None);
        let v: Value = serde_json::from_str(&s).unwrap();
        let sp = v["crabmate_tool"]["structured_payload"]
            .as_object()
            .expect("sp");
        assert_eq!(sp["schema"], "subprocess_exit_v1");
        assert_eq!(sp["exit_code"], 0);
        assert_eq!(sp["title"], "cargo check");
    }

    #[test]
    fn envelope_includes_structured_payload_http_fetch() {
        let raw = r#"method: GET
请求 URL: https://example.com/a
最终 URL: https://example.com/a
状态: 200 OK
Content-Type: text/plain

正文:
hi"#;
        let parsed = parse_legacy_output("http_fetch", raw);
        let s = encode_tool_message_envelope_v1("http_fetch", "s".into(), &parsed, raw, None);
        let v: Value = serde_json::from_str(&s).unwrap();
        let sp = v["crabmate_tool"]["structured_payload"]
            .as_object()
            .expect("sp");
        assert_eq!(sp["schema"], "http_tool_response_v1");
        assert_eq!(sp["http_status_code"], 200);
        assert_eq!(sp["method"], "GET");
        assert_eq!(sp["request_url"], "https://example.com/a");
    }

    #[test]
    fn structured_payload_http_skips_prefix_errors() {
        let raw = "错误：当前 URL 未匹配配置的 http_fetch_allowed_prefixes";
        assert!(structured_payload_for_tool("http_fetch", raw).is_none());
    }

    #[test]
    fn envelope_includes_structured_payload_run_command() {
        let raw = "命令：echo hi\n退出码：0\n标准输出：\nhi\n";
        let parsed = parse_legacy_output("run_command", raw);
        let s = encode_tool_message_envelope_v1("run_command", "s".into(), &parsed, raw, None);
        let v: Value = serde_json::from_str(&s).unwrap();
        let ct = v.get("crabmate_tool").unwrap();
        let sp = ct.get("structured_payload").expect("structured_payload");
        assert_eq!(sp["schema"], "run_command_exit_v1");
        assert_eq!(sp["exit_code"], 0);
        assert!(sp["stdout_nonempty"].as_bool() == Some(true));
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
        let path = root.join("src/cm_tools/fixtures/tool_result_envelope_golden.jsonl");
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
