//! `read_file` 参数解析（从 `read_tool.rs` 拆出以降低圈复杂度）。

use crate::text_encoding::parse_text_encoding_name;
use crate::tool_result::ToolError;

/// 单次 read_file 默认最多返回的行数（防撑爆上下文）
pub(super) const READ_FILE_DEFAULT_MAX_LINES: usize = 500;
/// read_file 允许的单次上限
pub(super) const READ_FILE_ABS_MAX_LINES: usize = 8000;
/// `anchor_line` 模式下未指定 `context_lines` 时每侧默认行数（对称窗口，仍受 `max_lines` 封顶）。
pub(super) const READ_FILE_ANCHOR_CONTEXT_DEFAULT: usize = 120;

#[must_use]
pub(super) fn compute_anchor_line_window(
    anchor_line: usize,
    context_lines: usize,
    max_lines: usize,
) -> (usize, usize) {
    let ml = max_lines.max(1);
    let max_half = ml.saturating_sub(1) / 2;
    let ctx_eff = context_lines.min(max_half);
    let span = (ctx_eff * 2 + 1).min(ml);
    let half_down = span.saturating_sub(1) / 2;
    let mut start_line = anchor_line.saturating_sub(half_down).max(1);
    let mut end_line = start_line.saturating_add(span.saturating_sub(1));
    if anchor_line > end_line {
        end_line = anchor_line;
        start_line = end_line.saturating_sub(span.saturating_sub(1)).max(1);
    }
    if anchor_line < start_line {
        start_line = anchor_line.max(1);
        end_line = start_line.saturating_add(span.saturating_sub(1));
    }
    (start_line, end_line)
}

pub(super) struct ReadFileParsedArgs {
    pub path: String,
    pub enc_name: crate::text_encoding::TextEncodingName,
    pub start_line: usize,
    pub end_line_opt: Option<usize>,
    pub max_lines: usize,
    pub count_total: bool,
}

#[allow(clippy::result_large_err)]
pub(super) fn parse_read_file_args(v: &serde_json::Value) -> Result<ReadFileParsedArgs, ToolError> {
    let path = match v.get("path").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => {
            return Err(ToolError::invalid_args("缺少 path 参数".to_string()));
        }
    };
    let enc_name = parse_text_encoding_name(v.get("encoding").and_then(|x| x.as_str()))
        .map_err(ToolError::invalid_args)?;

    let max_lines = v
        .get("max_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(READ_FILE_DEFAULT_MAX_LINES)
        .min(READ_FILE_ABS_MAX_LINES);

    let count_total = v
        .get("count_total_lines")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    if let Some(anchor_raw) = v.get("anchor_line") {
        return parse_anchor_branch(v, path, enc_name, max_lines, count_total, anchor_raw);
    }

    parse_range_branch(v, path, enc_name, max_lines, count_total)
}

#[allow(clippy::result_large_err)]
fn parse_anchor_branch(
    v: &serde_json::Value,
    path: String,
    enc_name: crate::text_encoding::TextEncodingName,
    max_lines: usize,
    count_total: bool,
    anchor_raw: &serde_json::Value,
) -> Result<ReadFileParsedArgs, ToolError> {
    let anchor_line = match anchor_raw.as_u64() {
        Some(n) if n >= 1 => n as usize,
        _ => {
            return Err(ToolError::invalid_args(
                "错误：anchor_line 必须是大于等于 1 的整数".to_string(),
            ));
        }
    };
    if v.get("start_line").is_some() || v.get("end_line").is_some() {
        return Err(ToolError::invalid_args(
            "错误：使用 anchor_line 时不要同时传 start_line/end_line（检索命中行号只用锚点即可对称取上下文）".to_string(),
        ));
    }
    let context_lines = v
        .get("context_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(READ_FILE_ANCHOR_CONTEXT_DEFAULT);
    let (start_line, end_line) = compute_anchor_line_window(anchor_line, context_lines, max_lines);
    Ok(ReadFileParsedArgs {
        path,
        enc_name,
        start_line,
        end_line_opt: Some(end_line),
        max_lines,
        count_total,
    })
}

#[allow(clippy::result_large_err)]
fn parse_range_branch(
    v: &serde_json::Value,
    path: String,
    enc_name: crate::text_encoding::TextEncodingName,
    max_lines: usize,
    count_total: bool,
) -> Result<ReadFileParsedArgs, ToolError> {
    let mut start_line = match v.get("start_line") {
        Some(n) => match n.as_u64() {
            Some(v) if v >= 1 => v as usize,
            _ => {
                return Err(ToolError::invalid_args(
                    "错误：start_line 必须是大于等于 1 的整数".to_string(),
                ));
            }
        },
        None => 1usize,
    };
    let mut end_line_opt = match v.get("end_line") {
        Some(n) => match n.as_u64() {
            Some(v) if v >= 1 => Some(v as usize),
            _ => {
                return Err(ToolError::invalid_args(
                    "错误：end_line 必须是大于等于 1 的整数".to_string(),
                ));
            }
        },
        None => None,
    };

    if let Some(e) = end_line_opt.as_mut()
        && *e < start_line
    {
        std::mem::swap(&mut start_line, e);
    }

    Ok(ReadFileParsedArgs {
        path,
        enc_name,
        start_line,
        end_line_opt,
        max_lines,
        count_total,
    })
}
