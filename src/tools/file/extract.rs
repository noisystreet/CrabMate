//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use regex::RegexBuilder;
use std::path::Path;

use super::path::{path_for_tool_display, resolve_for_read};

/// 在文件中按正则抽取匹配行（只读）。
/// 参数：
/// { "path": string, "pattern": string, "start_line"?: int, "end_line"?: int,
///   "max_matches"?: int, "case_insensitive"?: bool, "max_snippet_chars"?: int }
pub fn extract_in_file(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if path.is_empty() {
        return "缺少 path 参数".to_string();
    }

    let pattern = v
        .get("pattern")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let pattern = match pattern {
        Some(p) => p,
        None => return "缺少 pattern 参数".to_string(),
    };

    let start_line = v.get("start_line").and_then(|n| n.as_u64());
    let end_line = v.get("end_line").and_then(|n| n.as_u64());

    let start_line = match start_line {
        Some(n) if n >= 1 => Some(n as usize),
        Some(_) => return "错误：start_line 必须是大于等于 1 的整数".to_string(),
        None => None,
    };
    let end_line = match end_line {
        Some(n) if n >= 1 => Some(n as usize),
        Some(_) => return "错误：end_line 必须是大于等于 1 的整数".to_string(),
        None => None,
    };
    if let (Some(s), Some(e)) = (start_line, end_line)
        && e < s
    {
        return "错误：end_line 不能小于 start_line".to_string();
    }

    let max_matches = v
        .get("max_matches")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(50);
    let case_insensitive = v
        .get("case_insensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let max_snippet_chars = v
        .get("max_snippet_chars")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(400);
    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "lines".to_string());
    let max_block_chars = v
        .get("max_block_chars")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(8000);
    let max_block_lines = v
        .get("max_block_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(500);

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法读取".to_string();
    }

    let content = match std::fs::read_to_string(&target) {
        Ok(s) => s,
        Err(e) => return format!("读取文件失败: {}", e),
    };
    let all_lines: Vec<&str> = content.lines().collect();
    let total = all_lines.len();
    if total == 0 {
        return format!(
            "文件为空: {}",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }

    let from = start_line.unwrap_or(1);
    let to = end_line.unwrap_or(total);
    if from > total {
        return format!("错误：start_line 超出文件总行数（总行数: {}）", total);
    }
    let to = to.min(total);

    let re = match RegexBuilder::new(&pattern)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(r) => r,
        Err(e) => return format!("错误：无效的正则表达式：{}", e),
    };

    if mode == "lines" {
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
                path_for_tool_display(working_dir, &target, Some(&path)),
                from,
                to
            );
        }

        let mut out = String::new();
        out.push_str(&format!(
            "文件: {}\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n匹配结果（最多 {} 条，实际 {} 条）：\n",
            path_for_tool_display(working_dir, &target, Some(&path)),
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
        return out.trim_end().to_string();
    }

    if mode != "rust_fn_block" {
        return format!(
            "错误：不支持的 mode=\"{}\"（仅支持 \"lines\" 或 \"rust_fn_block\"）",
            mode
        );
    }

    // Rust 函数块提取：从匹配行开始找后续第一个 `{`，再按花括号配对抓到块结束。
    let mut blocks: Vec<(usize, usize, String)> = Vec::new(); // (start_line, end_line, text)
    for idx in from..=to {
        let line = all_lines[idx - 1];
        if !re.is_match(line) {
            continue;
        }

        let block =
            match extract_rust_brace_block(&all_lines, idx, max_block_lines, max_block_chars) {
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
            path_for_tool_display(working_dir, &target, Some(&path)),
            from,
            to
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "文件: {}\nmode: rust_fn_block\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n块结果（最多 {} 条，实际 {} 条）：\n",
        path_for_tool_display(working_dir, &target, Some(&path)),
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

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ScanState {
        Normal,
        LineComment,
        BlockComment,
        StringLit { escape: bool },
        CharLit { escape: bool },
        RawString { hash_count: usize },
    }

    let mut state = ScanState::Normal;
    let mut brace_count: i32 = 0;
    let mut started = false;
    let mut end_line: Option<usize> = None;

    // 扫描成本上限，避免极端文件导致非常大的扫描开销
    let mut char_budget: usize = max_block_chars.saturating_mul(3);

    for (line_idx, line) in all_lines.iter().enumerate().skip(start_idx) {
        if line_idx >= start_idx + max_block_lines || end_line.is_some() || char_budget == 0 {
            break;
        }

        let line = *line;
        let chars: Vec<char> = line.chars().collect();
        let mut pos: usize = 0;

        while pos < chars.len() {
            if char_budget == 0 {
                break;
            }
            let ch = chars[pos];
            char_budget = char_budget.saturating_sub(1);

            match state {
                ScanState::Normal => {
                    // // ... 直到行尾
                    if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
                        state = ScanState::LineComment;
                        pos += 2;
                        continue;
                    }
                    // /* ... */
                    if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '*' {
                        state = ScanState::BlockComment;
                        pos += 2;
                        continue;
                    }

                    // 原始字符串 r###" ... "###
                    if ch == 'r' || ch == 'R' {
                        // r" ... "
                        if pos + 1 < chars.len() && chars[pos + 1] == '"' {
                            state = ScanState::RawString { hash_count: 0 };
                            pos += 2;
                            continue;
                        }

                        // r#"... "#  /  r##"... "## ...
                        if pos + 1 < chars.len() && chars[pos + 1] == '#' {
                            let mut hash_count = 0usize;
                            let mut j = pos + 1;
                            while j < chars.len() && chars[j] == '#' {
                                hash_count += 1;
                                j += 1;
                            }
                            if j < chars.len() && chars[j] == '"' {
                                state = ScanState::RawString { hash_count };
                                pos = j + 1;
                                continue;
                            }
                        }
                    }

                    // 字符串
                    if ch == '"' {
                        state = ScanState::StringLit { escape: false };
                        pos += 1;
                        continue;
                    }
                    // 字符字面量
                    if ch == '\'' {
                        state = ScanState::CharLit { escape: false };
                        pos += 1;
                        continue;
                    }

                    // brace counting (只在 Normal 状态)
                    if !started {
                        if ch == '{' {
                            started = true;
                            brace_count = 1;
                        }
                    } else if ch == '{' {
                        brace_count += 1;
                    } else if ch == '}' {
                        brace_count -= 1;
                        if brace_count == 0 {
                            end_line = Some(line_idx);
                            break;
                        }
                    }

                    pos += 1;
                }
                ScanState::LineComment => {
                    // 跳过到行尾
                    break;
                }
                ScanState::BlockComment => {
                    if ch == '*' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
                        state = ScanState::Normal;
                        pos += 2;
                        continue;
                    }
                    pos += 1;
                }
                ScanState::StringLit { escape } => {
                    if escape {
                        state = ScanState::StringLit { escape: false };
                        pos += 1;
                        continue;
                    }
                    if ch == '\\' {
                        state = ScanState::StringLit { escape: true };
                        pos += 1;
                        continue;
                    }
                    if ch == '"' {
                        state = ScanState::Normal;
                        pos += 1;
                        continue;
                    }
                    pos += 1;
                }
                ScanState::CharLit { escape } => {
                    if escape {
                        state = ScanState::CharLit { escape: false };
                        pos += 1;
                        continue;
                    }
                    if ch == '\\' {
                        state = ScanState::CharLit { escape: true };
                        pos += 1;
                        continue;
                    }
                    if ch == '\'' {
                        state = ScanState::Normal;
                        pos += 1;
                        continue;
                    }
                    pos += 1;
                }
                ScanState::RawString { hash_count } => {
                    if ch == '"' {
                        // 检查后续是否为 hash_count 个 # 组成的结束定界符
                        let mut ok = true;
                        for k in 0..hash_count {
                            if pos + 1 + k >= chars.len() || chars[pos + 1 + k] != '#' {
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            state = ScanState::Normal;
                            pos = pos + 1 + hash_count;
                            continue;
                        }
                    }
                    pos += 1;
                }
            }
        }

        // // ... 在下一行会自动回到 Normal
        if state == ScanState::LineComment {
            state = ScanState::Normal;
        }
    }

    let end_line = match end_line {
        Some(e) => e,
        None => return Ok(None),
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
