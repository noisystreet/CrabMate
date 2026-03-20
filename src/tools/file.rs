//! 工作区文件创建与修改工具
//!
//! 路径均为**相对于工作目录**的相对路径（与 main 中 workspace 文件 API 一致，基于 run_command_working_dir）。

use std::path::{Path, PathBuf};
use regex::{RegexBuilder};

/// 解析用于读取或修改的路径（目标必须存在；path 必须为相对工作目录的相对路径）
fn resolve_for_read(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let joined = base.join(sub);
    joined
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))
}

/// 解析用于写入的路径（目标可不存在；path 必须为相对工作目录的相对路径，且不能通过 .. 超出工作目录）
fn resolve_for_write(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))?;
    let joined = base_canonical.join(sub);
    // 规范化 .. 和 . 并确保仍在 base 下（路径穿越检查）
    let normalized = normalize_path(&joined);
    if !normalized.starts_with(&base_canonical) {
        return Err("路径不能超出工作目录".to_string());
    }
    Ok(normalized)
}

/// 简单规范化：去掉 . 和 .. 段（不访问文件系统）
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// 创建文件：仅在文件不存在时创建；若已存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn create_file(args_json: &str, working_dir: &Path) -> String {
    let (path, content) = match parse_path_content(args_json) {
        Ok(pc) => pc,
        Err(e) => return e,
    };
    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if target.exists() {
        return "错误：文件已存在，无法仅创建".to_string();
    }
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return format!("创建目录失败: {}", e);
            }
        }
    }
    match std::fs::write(&target, content.as_bytes()) {
        Ok(()) => format!("已创建文件: {}", target.display()),
        Err(e) => format!("写入文件失败: {}", e),
    }
}

/// 修改文件：仅在文件已存在时覆盖内容；若不存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn modify_file(args_json: &str, working_dir: &Path) -> String {
    let (path, content) = match parse_path_content(args_json) {
        Ok(pc) => pc,
        Err(e) => return e,
    };
    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法仅修改".to_string();
    }
    match std::fs::write(&target, content.as_bytes()) {
        Ok(()) => format!("已修改文件: {}", target.display()),
        Err(e) => format!("写入文件失败: {}", e),
    }
}

/// 读取文件：仅当文件已存在时读取；支持 start_line/end_line 区间（1-based，含边界）。
/// 参数 args_json: { "path": string, "start_line"?: integer, "end_line"?: integer }
pub fn read_file(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let start_line = match v.get("start_line") {
        Some(n) => match n.as_u64() {
            Some(v) if v >= 1 => Some(v as usize),
            _ => return "错误：start_line 必须是大于等于 1 的整数".to_string(),
        },
        None => None,
    };
    let end_line = match v.get("end_line") {
        Some(n) => match n.as_u64() {
            Some(v) if v >= 1 => Some(v as usize),
            _ => return "错误：end_line 必须是大于等于 1 的整数".to_string(),
        },
        None => None,
    };
    if let (Some(s), Some(e)) = (start_line, end_line) {
        if e < s {
            return "错误：end_line 不能小于 start_line".to_string();
        }
    }

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
    if content.is_empty() {
        return format!("文件为空: {}", target.display());
    }

    let all_lines: Vec<&str> = content.lines().collect();
    let total = all_lines.len();
    let from = start_line.unwrap_or(1);
    let to = end_line.unwrap_or(total);
    if from > total {
        return format!("错误：start_line 超出文件总行数（总行数: {}）", total);
    }
    let to = to.min(total);

    let mut out = String::new();
    out.push_str(&format!(
        "文件: {}\n行范围: {}-{} / 总行数 {}\n",
        target.display(),
        from,
        to,
        total
    ));
    for idx in from..=to {
        let line = all_lines[idx - 1];
        out.push_str(&format!("{}|{}\n", idx, line));
    }
    out.trim_end().to_string()
}

/// 读取目录：返回指定目录下的文件/子目录列表（受控只读）。
/// 参数：{ "path"?: string, "max_entries"?: integer, "include_hidden"?: boolean }
pub fn read_dir(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    if path.starts_with('/') || path.contains("..") {
        return "错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径".to_string();
    }

    let max_entries = v
        .get("max_entries")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(200);
    let include_hidden = v.get("include_hidden").and_then(|b| b.as_bool()).unwrap_or(false);

    let root = match resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("错误：无法解析目录路径：{}", e),
    };
    if !root.is_dir() {
        return format!("错误：指定路径不是目录：{}", root.display());
    }

    let mut out = String::new();
    out.push_str(&format!("目录: {}\n", root.display()));
    match std::fs::read_dir(&root) {
        Ok(rd) => {
            let mut count = 0usize;
            let mut shown = 0usize;
            // 先遍历计数与展示（受 max_entries 限制）
            let mut entries: Vec<(String, bool)> = Vec::new();
            for e in rd.flatten() {
                count += 1;
                let name = e.file_name().to_string_lossy().to_string();
                if !include_hidden && name.starts_with('.') {
                    continue;
                }
                let is_dir = e.metadata().map(|m| m.is_dir()).unwrap_or(false);
                entries.push((name, is_dir));
            }
            entries.sort_by(|a, b| match (a.1, b.1) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
            });
            for (name, is_dir) in entries.into_iter().take(max_entries) {
                shown += 1;
                out.push_str(&format!("{}{}\n", if is_dir { "dir: " } else { "file: " }, name));
            }
            out.push_str(&format!("总计遍历: {}，展示: {}\n", count, shown));
            out.trim_end().to_string()
        }
        Err(e) => format!("读取目录失败：{}", e),
    }
}

/// 检查文件/目录是否存在。
/// 参数：{ "path": string, "kind"?: "file"|"dir"|"any" }
pub fn file_exists(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p,
        None => return "错误：缺少 path 参数".to_string(),
    };

    if path.starts_with('/') || path.contains("..") {
        return "错误：path 必须是工作区内相对路径，且不能包含 .. 或绝对路径".to_string();
    }

    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("any")
        .trim()
        .to_lowercase();

    let target = working_dir.join(path);
    let exists = target.exists();
    let type_ok = match kind.as_str() {
        "file" => target.is_file(),
        "dir" => target.is_dir(),
        "any" => exists,
        _ => return "错误：kind 仅支持 file|dir|any".to_string(),
    };

    let mut out = String::new();
    out.push_str(&format!("path: {}\n", path));
    out.push_str(&format!("exists: {}\n", exists));
    out.push_str(&format!("type_match: {}\n", type_ok));
    out.push_str(&format!("kind: {}\n", kind));
    out.trim_end().to_string()
}

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
    if let (Some(s), Some(e)) = (start_line, end_line) {
        if e < s {
            return "错误：end_line 不能小于 start_line".to_string();
        }
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
        return format!("文件为空: {}", target.display());
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
                target.display(),
                from,
                to
            );
        }

        let mut out = String::new();
        out.push_str(&format!(
            "文件: {}\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n匹配结果（最多 {} 条，实际 {} 条）：\n",
            target.display(),
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

        let block = match extract_rust_brace_block(&all_lines, idx, max_block_lines, max_block_chars)
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
            target.display(),
            from,
            to
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "文件: {}\nmode: rust_fn_block\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n块结果（最多 {} 条，实际 {} 条）：\n",
        target.display(),
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

    for line_idx in start_idx..all_lines.len() {
        if line_idx >= start_idx + max_block_lines || end_line.is_some() || char_budget == 0 {
            break;
        }

        let line = all_lines[line_idx];
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

fn parse_path_content(args_json: &str) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(String::from)
        .ok_or_else(|| "缺少 path 参数".to_string())?;
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    Ok((path, content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn make_test_dir() -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "crabmate_file_tool_test_{}_{}_{}",
            std::process::id(),
            ts,
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_read_file_with_line_range() {
        let dir = make_test_dir();
        let file = dir.join("a.txt");
        std::fs::write(&file, "a\nb\nc\nd\n").unwrap();
        let out = read_file(r#"{"path":"a.txt","start_line":2,"end_line":3}"#, &dir);
        assert!(out.contains("2|b"), "应包含第 2 行: {}", out);
        assert!(out.contains("3|c"), "应包含第 3 行: {}", out);
        assert!(!out.contains("1|a"), "不应包含第 1 行: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_reject_invalid_range() {
        let dir = make_test_dir();
        let file = dir.join("a.txt");
        std::fs::write(&file, "x\n").unwrap();
        let out = read_file(r#"{"path":"a.txt","start_line":3}"#, &dir);
        assert!(out.contains("超出文件总行数"), "应报越界错误: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_rust_fn_block() {
        let dir = make_test_dir();
        let file = dir.join("a.rs");
        let content = r##"
pub fn foo(x: i32) -> i32 {
    // braces in line comment: { }
    let s1 = "{";
    let s2 = "}";
    let s3 = r#"{"a":1}"#; // braces inside raw string
    let s4 = r#"}"#;        // raw string with '}' earlier than function end
    /* block comment with { and } should be ignored: { } */
    let c = '}';

    let _ = some_macro!({
        // comment with { } inside macro invocation should not break extraction
        println!("macro {{ }} {}", x);
        if x > 0 { x + 1 } else { x - 1 }
    });

    // The real return is still from the outer if/else, so braces above must not affect boundaries.
    if x > 0 {
        x + 1 // { in comment { }
    } else {
        x - 1
    }
}

pub fn bar() { println!("hi"); }
"##;
        std::fs::write(&file, content).unwrap();

        let out = extract_in_file(
            r#"{"path":"a.rs","pattern":"pub\\s+fn\\s+foo","mode":"rust_fn_block","max_matches":1,"max_block_lines":200,"max_block_chars":2000}"#,
            &dir,
        );
        assert!(out.contains("pub fn foo"));
        assert!(out.contains("else"));
        assert!(out.contains("x - 1"));
        assert!(out.contains("let s4 = r#\"}\"#;"));
        assert!(out.trim_end().ends_with('}'));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
