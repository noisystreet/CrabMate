//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use regex::{Regex, RegexBuilder};
use std::path::Path;

use crate::cm_tools::text_encoding::{decode_bytes_strict, parse_text_encoding_name};

use super::path::{path_for_tool_display, resolve_for_read, tool_user_error_from_workspace_path};
use super::rust_brace_scan::{
    RustBraceLineStep, RustBraceScanCtx, RustBraceScanState, rust_brace_scan_step,
};

struct ExtractInFileParams {
    path: String,
    enc_name: crate::cm_tools::text_encoding::TextEncodingName,
    pattern: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
    max_matches: usize,
    case_insensitive: bool,
    max_snippet_chars: usize,
    mode: String,
    max_block_chars: usize,
    max_block_lines: usize,
}

fn json_u64_min1(v: &serde_json::Value, key: &str, default: usize) -> usize {
    v.get(key)
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(default)
}

fn parse_optional_line_1based(v: &serde_json::Value, key: &str) -> Result<Option<usize>, String> {
    match v.get(key).and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => Ok(Some(n as usize)),
        Some(_) => Err(format!("错误：{key} 必须是大于等于 1 的整数")),
        None => Ok(None),
    }
}

fn parse_extract_in_file_params(v: &serde_json::Value) -> Result<ExtractInFileParams, String> {
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if path.is_empty() {
        return Err("缺少 path 参数".to_string());
    }
    let enc_name = parse_text_encoding_name(v.get("encoding").and_then(|x| x.as_str()))?;

    let pattern = v
        .get("pattern")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| "缺少 pattern 参数".to_string())?;

    let start_line = parse_optional_line_1based(v, "start_line")?;
    let end_line = parse_optional_line_1based(v, "end_line")?;
    if let (Some(s), Some(e)) = (start_line, end_line)
        && e < s
    {
        return Err("错误：end_line 不能小于 start_line".to_string());
    }

    let max_matches = json_u64_min1(v, "max_matches", 50);
    let case_insensitive = v
        .get("case_insensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let max_snippet_chars = json_u64_min1(v, "max_snippet_chars", 400);
    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "lines".to_string());
    let max_block_chars = json_u64_min1(v, "max_block_chars", 8000);
    let max_block_lines = json_u64_min1(v, "max_block_lines", 500);

    Ok(ExtractInFileParams {
        path,
        enc_name,
        pattern,
        start_line,
        end_line,
        max_matches,
        case_insensitive,
        max_snippet_chars,
        mode,
        max_block_chars,
        max_block_lines,
    })
}

/// 行模式匹配输出（与拆分前 `extract_in_file` 的 `mode=lines` 分支一致）。
struct ExtractInFileLinesModeParams<'a> {
    working_dir: &'a Path,
    path: &'a str,
    target: &'a Path,
    all_lines: &'a [&'a str],
    total: usize,
    from: usize,
    to: usize,
    re: &'a Regex,
    pattern: &'a str,
    max_matches: usize,
    max_snippet_chars: usize,
}

fn extract_in_file_lines_mode(p: ExtractInFileLinesModeParams<'_>) -> String {
    let ExtractInFileLinesModeParams {
        working_dir,
        path,
        target,
        all_lines,
        total,
        from,
        to,
        re,
        pattern,
        max_matches,
        max_snippet_chars,
    } = p;
    let mut matches: Vec<(usize, String)> = Vec::new();
    for idx in from..=to {
        let line = all_lines[idx - 1];
        if re.is_match(line) {
            matches.push((idx, truncate_line(line, max_snippet_chars)));
            if matches.len() >= max_matches {
                break;
            }
        }
    }

    if matches.is_empty() {
        return format!(
            "未找到匹配：pattern=\"{}\"（文件: {}, 行范围 {}-{}）",
            pattern,
            path_for_tool_display(working_dir, target, Some(path)),
            from,
            to
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "文件: {}\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n匹配结果（最多 {} 条，实际 {} 条）：\n",
        path_for_tool_display(working_dir, target, Some(path)),
        pattern,
        from,
        to,
        total,
        max_matches,
        matches.len()
    ));
    for (line_no, line) in matches {
        out.push_str(&format!("{}|{}\n", line_no, line));
    }
    out.trim_end().to_string()
}

/// `rust_fn_block` 模式（与拆分前一致）。
struct ExtractInFileRustFnBlockParams<'a> {
    working_dir: &'a Path,
    path: &'a str,
    target: &'a Path,
    all_lines: &'a [&'a str],
    total: usize,
    from: usize,
    to: usize,
    re: &'a Regex,
    pattern: &'a str,
    max_matches: usize,
    max_block_lines: usize,
    max_block_chars: usize,
}

fn extract_in_file_rust_fn_block_mode(p: ExtractInFileRustFnBlockParams<'_>) -> String {
    let ExtractInFileRustFnBlockParams {
        working_dir,
        path,
        target,
        all_lines,
        total,
        from,
        to,
        re,
        pattern,
        max_matches,
        max_block_lines,
        max_block_chars,
    } = p;
    let mut blocks: Vec<(usize, usize, String)> = Vec::new();
    for idx in from..=to {
        let line = all_lines[idx - 1];
        if !re.is_match(line) {
            continue;
        }

        let block = match extract_rust_brace_block(all_lines, idx, max_block_lines, max_block_chars)
        {
            Ok(Some((s, e, txt))) => (s, e, txt),
            Ok(None) => continue,
            Err(e) => return e,
        };
        if blocks.len() >= max_matches {
            break;
        }
        blocks.push(block);
    }

    if blocks.is_empty() {
        return format!(
            "未找到 Rust 代码块：pattern=\"{}\"（文件: {}, 行范围 {}-{}）",
            pattern,
            path_for_tool_display(working_dir, target, Some(path)),
            from,
            to
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "文件: {}\nmode: rust_fn_block\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n块结果（最多 {} 条，实际 {} 条）：\n",
        path_for_tool_display(working_dir, target, Some(path)),
        pattern,
        from,
        to,
        total,
        max_matches,
        blocks.len()
    ));
    for (s, e, txt) in blocks {
        out.push_str(&format!("block: {}-{}\n", s, e));
        out.push_str(&format!("{}\n", txt));
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn load_extract_file_text(
    working_dir: &Path,
    path: &str,
    enc_name: crate::cm_tools::text_encoding::TextEncodingName,
) -> Result<(std::path::PathBuf, String), String> {
    let target = match resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return Err(tool_user_error_from_workspace_path(e)),
    };
    if !target.is_file() {
        return Err("错误：路径不是文件或不存在，无法读取".to_string());
    }
    let raw = std::fs::read(&target).map_err(|e| format!("读取文件失败: {}", e))?;
    match decode_bytes_strict(&raw, enc_name) {
        Ok((s, _note)) => Ok((target, s)),
        Err(e) => Err(e),
    }
}

fn extract_line_window(
    total: usize,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> Result<(usize, usize), String> {
    let from = start_line.unwrap_or(1);
    let to = end_line.unwrap_or(total);
    if from > total {
        return Err(format!(
            "错误：start_line 超出文件总行数（总行数: {}）",
            total
        ));
    }
    Ok((from, to.min(total)))
}

fn compile_extract_regex(pattern: &str, case_insensitive: bool) -> Result<Regex, String> {
    RegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
        .map_err(|e| format!("错误：无效的正则表达式：{}", e))
}

/// 在文件中按正则抽取匹配行（只读）。
/// 参数：
/// { "path": string, "pattern": string, "start_line"?: int, "end_line"?: int,
///   "max_matches"?: int, "case_insensitive"?: bool, "max_snippet_chars"?: int }
pub fn extract_in_file(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let p = match parse_extract_in_file_params(&v) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let ExtractInFileParams {
        path,
        enc_name,
        pattern,
        start_line,
        end_line,
        max_matches,
        case_insensitive,
        max_snippet_chars,
        mode,
        max_block_chars,
        max_block_lines,
    } = p;

    let (target, content) = match load_extract_file_text(working_dir, &path, enc_name) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let all_lines: Vec<&str> = content.lines().collect();
    let total = all_lines.len();
    if total == 0 {
        return format!(
            "文件为空: {}",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }

    let (from, to) = match extract_line_window(total, start_line, end_line) {
        Ok(w) => w,
        Err(e) => return e,
    };

    let re = match compile_extract_regex(&pattern, case_insensitive) {
        Ok(r) => r,
        Err(e) => return e,
    };

    if mode == "lines" {
        return extract_in_file_lines_mode(ExtractInFileLinesModeParams {
            working_dir,
            path: path.as_str(),
            target: &target,
            all_lines: &all_lines,
            total,
            from,
            to,
            re: &re,
            pattern: pattern.as_str(),
            max_matches,
            max_snippet_chars,
        });
    }

    if mode != "rust_fn_block" {
        return format!(
            "错误：不支持的 mode=\"{}\"（仅支持 \"lines\" 或 \"rust_fn_block\"）",
            mode
        );
    }

    extract_in_file_rust_fn_block_mode(ExtractInFileRustFnBlockParams {
        working_dir,
        path: path.as_str(),
        target: &target,
        all_lines: &all_lines,
        total,
        from,
        to,
        re: &re,
        pattern: pattern.as_str(),
        max_matches,
        max_block_lines,
        max_block_chars,
    })
}

fn truncate_line(s: &str, max_chars: usize) -> String {
    let s = s.trim_end();
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i >= max_chars {
                break;
            }
            out.push(ch);
        }
        format!("{}... (截断)", out)
    }
}

fn scan_rust_brace_line(
    line: &str,
    line_idx: usize,
    state: &mut RustBraceScanState,
    started: &mut bool,
    brace_count: &mut i32,
    end_line: &mut Option<usize>,
    char_budget: &mut usize,
) {
    let chars: Vec<char> = line.chars().collect();
    let mut pos: usize = 0;
    let mut scan_ctx = RustBraceScanCtx {
        line_idx,
        chars: &chars,
        started,
        brace_count,
        end_line,
    };

    while pos < chars.len() {
        if *char_budget == 0 {
            break;
        }
        let ch = chars[pos];
        *char_budget = char_budget.saturating_sub(1);

        match rust_brace_scan_step(*state, pos, ch, &mut scan_ctx) {
            RustBraceLineStep::Continue { state: ns, pos: np } => {
                *state = ns;
                pos = np;
            }
            RustBraceLineStep::BreakCharLoop | RustBraceLineStep::BreakLineScan => break,
        }
    }

    if *state == RustBraceScanState::LineComment {
        *state = RustBraceScanState::Normal;
    }
}

/// 从 start_line（1-based）开始向后提取 `{ ... }` 配对块。
/// 说明：会在扫描时跳过注释/字符串/原始字符串/字符字面量里的 `{`/`}`，
/// 以避免花括号误判块边界。
fn extract_rust_brace_block(
    all_lines: &[&str],
    start_line_1based: usize,
    max_block_lines: usize,
    max_block_chars: usize,
) -> Result<Option<(usize, usize, String)>, String> {
    if start_line_1based == 0 {
        return Ok(None);
    }
    let start_idx = start_line_1based - 1;
    if start_idx >= all_lines.len() {
        return Ok(None);
    }

    let mut state = RustBraceScanState::Normal;
    let mut brace_count: i32 = 0;
    let mut started = false;
    let mut end_line: Option<usize> = None;
    let mut char_budget: usize = max_block_chars.saturating_mul(3);

    for (line_idx, line) in all_lines.iter().enumerate().skip(start_idx) {
        if line_idx >= start_idx + max_block_lines || end_line.is_some() || char_budget == 0 {
            break;
        }
        scan_rust_brace_line(
            line,
            line_idx,
            &mut state,
            &mut started,
            &mut brace_count,
            &mut end_line,
            &mut char_budget,
        );
    }

    let Some(end_line) = end_line else {
        return Ok(None);
    };

    let text = all_lines[start_idx..=end_line].join("\n");
    let text_trunc = truncate_by_chars(&text, max_block_chars);
    Ok(Some((start_line_1based, end_line + 1, text_trunc)))
}

fn truncate_by_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i >= max_chars {
                break;
            }
            out.push(ch);
        }
        format!("{}... (截断)", out)
    }
}
