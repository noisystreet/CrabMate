//! `message_display` 中 JSON 工具信封（`NormalizedToolEnvelope`）的展示辅助。

use crabmate_tools::tool_result::NormalizedToolEnvelope;

pub const TOOL_OUTPUT_SECTION_HEADLINE: &str = "### 执行输出";

fn normalized_tool_output_trunc_note(env: &NormalizedToolEnvelope) -> Option<String> {
    if !env.output_truncated {
        return None;
    }
    let orig = env
        .output_original_chars
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    let head = env
        .output_kept_head_chars
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    let tail = env
        .output_kept_tail_chars
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    Some(format!(
        "（输出已压缩入上下文：原文约 {orig} 字符，保留首尾约 {head}+{tail} 字符；见 `output` 内采样与说明。）"
    ))
}

fn normalized_tool_struct_note(env: &NormalizedToolEnvelope) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if env.execution_mode.as_deref() == Some("parallel_readonly_batch")
        && let Some(ref bid) = env.parallel_batch_id
        && !bid.is_empty()
    {
        parts.push(format!("并行只读批次 `{bid}`"));
    }
    if env.retryable == Some(true) {
        parts.push("失败可能可重试（启发式 `retryable`）".to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(format!("（{}）", parts.join("；")))
    }
}

fn join_normalized_tool_note_lines(
    trunc_note: Option<String>,
    struct_note: Option<String>,
) -> Option<String> {
    let mut note_lines: Vec<String> = Vec::new();
    if let Some(n) = trunc_note.filter(|s| !s.is_empty()) {
        note_lines.push(n);
    }
    if let Some(n) = struct_note.filter(|s| !s.is_empty()) {
        note_lines.push(n);
    }
    if note_lines.is_empty() {
        None
    } else {
        Some(note_lines.join("\n"))
    }
}

fn tool_display_normalized_envelope_with_raw(
    v: &serde_json::Value,
    t: &str,
    summary: &str,
    combined_note: Option<&String>,
) -> String {
    let pretty = serde_json::to_string_pretty(v).unwrap_or_else(|_| t.to_string());
    match (summary.is_empty(), combined_note) {
        (true, Some(note)) if !note.is_empty() => {
            format!("{note}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}")
        }
        (true, _) => format!("{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}"),
        (false, Some(note)) if !note.is_empty() => {
            format!("{summary}\n{note}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}")
        }
        (false, _) => format!("{summary}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}"),
    }
}

fn tool_display_normalized_envelope_summary_only(
    summary: &str,
    combined_note: Option<&String>,
) -> String {
    if summary.is_empty() {
        return combined_note.cloned().unwrap_or_default();
    }
    match combined_note {
        Some(note) if !note.is_empty() => format!("{summary}\n{note}"),
        _ => summary.to_string(),
    }
}

pub fn tool_display_from_normalized_envelope(
    v: &serde_json::Value,
    t: &str,
    env: &NormalizedToolEnvelope,
    include_raw: bool,
) -> String {
    let combined_note = join_normalized_tool_note_lines(
        normalized_tool_output_trunc_note(env),
        normalized_tool_struct_note(env),
    );
    let summary = env.summary.trim();
    if include_raw {
        return tool_display_normalized_envelope_with_raw(v, t, summary, combined_note.as_ref());
    }
    tool_display_normalized_envelope_summary_only(summary, combined_note.as_ref())
}
