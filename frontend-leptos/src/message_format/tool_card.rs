//! 工具结果卡片的展示用单行/多行摘要（与 SSE `ToolResultInfo` 对齐）。

use crate::i18n::{self, Locale};
use crate::sse_dispatch::ToolResultInfo;

use super::plain::collapse_duplicate_summary_lines;

const COMPACT_SEPARATOR: &str = " ｜ ";

fn join_compact_parts(left: &str, right: &str) -> String {
    format!("{left}{COMPACT_SEPARATOR}{right}")
}

fn first_nonempty_line_with_prefix<'a>(output: &'a str, prefix: &str) -> Option<&'a str> {
    for line in output.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix(prefix) {
            let v = v.trim();
            if !v.is_empty() {
                return Some(v);
            }
        }
        break;
    }
    None
}

fn compact_path_prefix_from_output(info: &ToolResultInfo) -> Option<String> {
    let v = first_nonempty_line_with_prefix(&info.output, "路径：")?;
    // 标题已表明创建/修改文件，不再重复「路径 ｜ …」。
    Some(v.to_string())
}

fn compact_from_to_from_output(info: &ToolResultInfo) -> Option<String> {
    for line in info.output.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("从→到：") {
            let rest = rest.trim();
            if !rest.is_empty() {
                // 标题已表明复制/移动，紧凑条只保留「源 → 目标」。
                return Some(rest.to_string());
            }
        }
        break;
    }
    None
}

fn compact_search_from_tool_output(output: &str, loc: Locale) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lines: Vec<&str> = trimmed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return None;
    }
    let mut idx = 0usize;
    let mut match_count: Option<u64> = None;
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(lines[0]) {
        if v.get("kind").and_then(|x| x.as_str()) == Some("crabmate_tool_output")
            && v.get("tool").and_then(|x| x.as_str()) == Some("search_in_files")
        {
            match_count = v.get("match_count").and_then(|x| x.as_u64());
            idx = 1;
        }
    }
    let mut pat: Option<String> = None;
    let mut scope: Option<String> = None;
    for line in lines.iter().skip(idx) {
        if let Some(v) = line.strip_prefix("搜索：") {
            pat = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = line.strip_prefix("范围：") {
            scope = Some(v.trim().to_string());
        }
    }
    let pat = pat?;
    let sc = scope.unwrap_or_else(|| ".".to_string());
    let mut right = format!("{pat} · {sc}");
    if let Some(n) = match_count.filter(|&n| n > 0) {
        right.push_str(" · ");
        right.push_str(&i18n::tool_search_compact_hits_suffix(loc, n as usize));
    }
    Some(join_compact_parts(
        i18n::tool_search_compact_header_label(loc),
        &right,
    ))
}

/// 后端摘要为 `read file: <path>`（见 `tool_summary_args::ReadFileSummaryArgs`），与中文「路径：」并存。
fn path_from_summary_read_file_english(joined: &str) -> Option<String> {
    let needle = "read file:";
    let idx = joined.find(needle)?;
    let rest = joined[idx + needle.len()..].trim();
    if rest.is_empty() {
        return None;
    }
    let end = rest
        .find(" [")
        .or_else(|| rest.find(" enc="))
        .unwrap_or(rest.len());
    let path = rest[..end].trim();
    if path.is_empty() {
        None
    } else {
        Some(path.to_string())
    }
}

fn compact_read_file_from_output(output: &str, loc: Locale) -> Option<String> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        return None;
    }
    let first = trimmed.lines().next()?.trim();
    let v: serde_json::Value = serde_json::from_str(first).ok()?;
    if v.get("kind").and_then(|x| x.as_str()) != Some("crabmate_tool_output") {
        return None;
    }
    if v.get("tool").and_then(|x| x.as_str()) != Some("read_file") {
        return None;
    }
    let path = v.get("path").and_then(|x| x.as_str())?.trim();
    if path.is_empty() {
        return None;
    }
    let total = v.get("total_lines").and_then(|x| x.as_u64());
    let returned = v.get("line_count_returned").and_then(|x| x.as_u64());
    let line_count = match (total, returned) {
        (Some(t), _) if t > 0 => Some(t as usize),
        (None, Some(r)) if r > 0 => Some(r as usize),
        (Some(0), Some(r)) if r > 0 => Some(r as usize),
        _ => None,
    };
    Some(match line_count {
        Some(n) => join_compact_parts(path, &i18n::tool_read_file_lines_suffix(loc, n)),
        None => path.to_string(),
    })
}

fn strip_tool_status_prefix(line: &str) -> String {
    let trimmed = line.trim();
    for prefix in ["✅ ", "❌ ", "🟡 "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest.trim().to_string();
        }
    }
    trimmed.to_string()
}

fn rewrite_legacy_tool_summary(sum: &str, loc: Locale) -> String {
    let normalized = i18n::tool_summary_normalize_line_breaks(sum, loc);
    let mut lines: Vec<String> = normalized
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    if lines.is_empty() {
        return String::new();
    }

    let first_raw = lines.remove(0);
    let first = strip_tool_status_prefix(&first_raw);
    let mut title = first.clone();
    if let Some((left, right)) = first.split_once(i18n::tool_cmd_success_sep(loc)) {
        let tool = left.trim();
        let rest = right.trim();
        title = i18n::tool_rewrite_title_done(loc, &i18n::tool_human_name(loc, tool));
        if !rest.is_empty() {
            lines.insert(0, rest.to_string());
        }
    } else if let Some((left, right)) = first.split_once(i18n::tool_cmd_fail_sep(loc)) {
        let tool = left.trim();
        let rest = right.trim();
        title = i18n::tool_rewrite_title_failed_run(loc, &i18n::tool_human_name(loc, tool));
        if !rest.is_empty() {
            lines.insert(0, rest.to_string());
        }
    }

    let mut extras: Vec<String> = Vec::new();
    for line in lines {
        if line == i18n::tool_exit_line_zero(loc)
            || line == i18n::tool_exit_line_zero(Locale::ZhHans)
        {
            continue;
        }
        if let Some(v) = line
            .strip_prefix(i18n::tool_line_stdout_prefix(loc))
            .or_else(|| line.strip_prefix(i18n::tool_line_stdout_prefix(Locale::ZhHans)))
        {
            let v = v.trim();
            if !v.is_empty() {
                let label = i18n::tool_summary_label_stdout(loc);
                extras.push(format!("{label}：{v}"));
            }
            continue;
        }
        if let Some(v) = line
            .strip_prefix(i18n::tool_line_stderr_prefix(loc))
            .or_else(|| line.strip_prefix(i18n::tool_line_stderr_prefix(Locale::ZhHans)))
        {
            let v = v.trim();
            if !v.is_empty() {
                let label = i18n::tool_summary_label_stderr(loc);
                extras.push(format!("{label}：{v}"));
            }
            continue;
        }
        if line.starts_with(i18n::tool_line_exit_prefix(loc))
            || line.starts_with(i18n::tool_line_exit_prefix(Locale::ZhHans))
        {
            extras.push(line.to_string());
            continue;
        }
        extras.push(line);
    }

    if extras.is_empty() {
        return title;
    }
    format!("{title}\n\n{}", extras.join("\n"))
}

#[inline]
fn tool_name_human(name: &str, loc: Locale) -> String {
    i18n::tool_human_name(loc, name)
}

fn compact_key_signal(info: &ToolResultInfo, summary: &str, loc: Locale) -> Option<String> {
    match info.name.trim() {
        "run_command" => {
            for line in info.output.lines().map(str::trim) {
                if line.is_empty() {
                    continue;
                }
                if let Some(inv) = line.strip_prefix("命令：") {
                    let inv = inv.trim();
                    if !inv.is_empty() {
                        // 标题已是「命令执行 / Command run」，不再重复「命令 ｜ …」字段标签。
                        return Some(inv.to_string());
                    }
                }
                break;
            }
        }
        "create_file" | "modify_file" => {
            if let Some(s) = compact_path_prefix_from_output(info) {
                return Some(s);
            }
        }
        "copy_file" | "move_file" => {
            if let Some(s) = compact_from_to_from_output(info) {
                return Some(s);
            }
        }
        "search_in_files" => {
            if let Some(s) = compact_search_from_tool_output(&info.output, loc) {
                return Some(s);
            }
        }
        "read_file" => {
            if let Some(s) = compact_read_file_from_output(&info.output, loc) {
                return Some(s);
            }
        }
        _ => {}
    }

    let lines = summary
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    if info.name.trim() == "read_dir" {
        let joined = lines.join(" ");
        let dir = joined
            .split(i18n::tool_read_dir_label_dir(loc))
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .or_else(|| {
                joined
                    .split(i18n::tool_read_dir_label_dir(Locale::ZhHans))
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .unwrap_or(".");
        let shown = joined
            .split(i18n::tool_read_dir_label_shown(loc))
            .nth(1)
            .and_then(|rest| {
                rest.chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<usize>()
                    .ok()
            })
            .unwrap_or(0);
        // 标题已是「读取目录 / Read directory」，不再重复「目录 ｜ …」。
        return Some(join_compact_parts(
            dir,
            &i18n::tool_read_dir_compact_entries(loc, shown),
        ));
    }
    if info.name.trim() == "read_file" {
        let joined = lines.join(" ");
        let path = path_from_summary_read_file_english(&joined)
            .or_else(|| {
                joined
                    .split(i18n::tool_read_file_label_path(loc))
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(str::to_string)
            })
            .or_else(|| {
                joined
                    .split(i18n::tool_read_file_label_path(Locale::ZhHans))
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(str::to_string)
            })
            .or_else(|| {
                joined
                    .split("path:")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| i18n::tool_read_file_default_path(loc).to_string());
        let line_count = joined
            .split(i18n::tool_read_file_label_lines(loc))
            .nth(1)
            .and_then(|rest| {
                rest.chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<usize>()
                    .ok()
            })
            .or_else(|| {
                joined.split("lines:").nth(1).and_then(|rest| {
                    rest.chars()
                        .skip_while(|c| c.is_whitespace())
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse::<usize>()
                        .ok()
                })
            })
            .or_else(|| {
                joined
                    .split(i18n::tool_read_file_label_lines(Locale::ZhHans))
                    .nth(1)
                    .and_then(|rest| {
                        rest.chars()
                            .skip_while(|c| c.is_whitespace())
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .parse::<usize>()
                            .ok()
                    })
            });
        return Some(match line_count {
            Some(n) => join_compact_parts(&path, &i18n::tool_read_file_lines_suffix(loc, n)),
            None => path,
        });
    }
    if info.name.trim() == "search_in_files" {
        let joined = lines.join(" ");
        let keyword = joined
            .split(i18n::tool_search_label_keyword(loc))
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .or_else(|| {
                joined
                    .split(i18n::tool_search_label_keyword(Locale::ZhHans))
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .or_else(|| {
                joined
                    .split("pattern:")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .unwrap_or(i18n::tool_search_default_keyword(loc));
        let hit_count = joined
            .split(i18n::tool_search_label_hits(loc))
            .nth(1)
            .and_then(|rest| {
                rest.chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<usize>()
                    .ok()
            })
            .or_else(|| {
                joined.split("hits:").nth(1).and_then(|rest| {
                    rest.chars()
                        .skip_while(|c| c.is_whitespace())
                        .take_while(|c| c.is_ascii_digit())
                        .collect::<String>()
                        .parse::<usize>()
                        .ok()
                })
            })
            .or_else(|| {
                joined
                    .split(i18n::tool_search_label_hits(Locale::ZhHans))
                    .nth(1)
                    .and_then(|rest| {
                        rest.chars()
                            .skip_while(|c| c.is_whitespace())
                            .take_while(|c| c.is_ascii_digit())
                            .collect::<String>()
                            .parse::<usize>()
                            .ok()
                    })
            });
        return Some(match hit_count {
            Some(n) => join_compact_parts(
                &format!("{} {keyword}", i18n::tool_search_compact_keyword_word(loc)),
                &i18n::tool_search_compact_hits_suffix(loc, n),
            ),
            None => format!("{} {keyword}", i18n::tool_search_compact_keyword_word(loc)),
        });
    }
    lines
        .iter()
        .find(|l| i18n::summary_line_looks_like_compact_signal(l, loc))
        .map(|s| s.to_string())
        .or_else(|| lines.first().map(|s| (*s).to_string()))
}

fn render_tool_title(info: &ToolResultInfo, loc: Locale) -> String {
    let human = tool_name_human(info.name.trim(), loc);
    let ok = info.ok.unwrap_or(true);
    if ok {
        i18n::tool_title_completed(loc, &human)
    } else {
        i18n::tool_title_failed(loc, &human)
    }
}

fn build_tool_failure_suggestion(info: &ToolResultInfo, loc: Locale) -> Option<String> {
    if info.ok.unwrap_or(true) {
        return None;
    }
    let code = info
        .error_code
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let hint = if code.contains("timeout") {
        i18n::tool_failure_suggest_timeout(loc)
    } else if code.contains("invalid") || code.contains("arg") {
        i18n::tool_failure_suggest_invalid_args(loc)
    } else {
        i18n::tool_failure_suggest_generic(loc)
    };
    Some(hint.to_string())
}

fn build_tool_failure_block(info: &ToolResultInfo, loc: Locale, body: &str) -> Option<String> {
    if info.ok.unwrap_or(true) {
        return None;
    }
    let happened = if !body.trim().is_empty() {
        body.trim().to_string()
    } else if let Some(code) = info.error_code.as_deref().filter(|s| !s.is_empty()) {
        i18n::tool_failure_returned_code(loc, code)
    } else {
        i18n::tool_failure_no_detail(loc).to_string()
    };
    let impact = if let Some(ec) = info.exit_code {
        i18n::tool_failure_impact_exit(loc, ec)
    } else {
        i18n::tool_failure_impact_no_exit(loc).to_string()
    };
    let suggestion = build_tool_failure_suggestion(info, loc)
        .unwrap_or_else(|| i18n::tool_failure_suggest_fallback(loc).to_string());
    Some(i18n::format_error_three_part(
        loc,
        happened.as_str(),
        impact.as_str(),
        suggestion.as_str(),
    ))
}

fn normalized_tool_summary(info: &ToolResultInfo, loc: Locale) -> String {
    let sum = info.summary.as_deref().unwrap_or("").trim();
    if sum.is_empty() {
        return String::new();
    }
    let sum = collapse_duplicate_summary_lines(sum);
    if sum.is_empty() {
        return String::new();
    }
    let mut lines = sum.lines();
    let first = lines.next().unwrap_or_default().trim().to_string();
    if first.is_empty() {
        return String::new();
    }
    let rest: Vec<&str> = lines
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != first.as_str())
        .collect();
    let mut out = if rest.is_empty() {
        first
    } else {
        let mut v = first;
        v.push_str("\n\n");
        v.push_str(&rest.join("\n"));
        v
    };
    let rewritten = rewrite_legacy_tool_summary(&out, loc);
    if !rewritten.is_empty() {
        out = rewritten;
    }
    out
}

/// 摘要首行若与紧凑标题相同（重写摘要已含工具人类名），合并为详情时去掉重复前缀。
fn summary_without_redundant_title(title: &str, summary: &str) -> String {
    let s = summary.trim();
    if s.is_empty() || s == title {
        return String::new();
    }
    for sep in ["\n\n", "\n"] {
        let prefix = format!("{title}{sep}");
        if let Some(rest) = s.strip_prefix(&prefix) {
            return rest.trim().to_string();
        }
    }
    s.to_string()
}

pub fn tool_card_compact_text(info: &ToolResultInfo, loc: Locale) -> String {
    let title = render_tool_title(info, loc);
    let summary = normalized_tool_summary(info, loc);
    let candidate = compact_key_signal(info, &summary, loc);
    let mut out = title.clone();
    let skip_compact = i18n::tool_card_compact_skip_headings(loc);
    if let Some(c) = candidate.as_deref()
        && !c.is_empty()
        && c != title
        && !skip_compact.contains(&c)
    {
        out.push_str(COMPACT_SEPARATOR);
        out.push_str(c);
    }
    if !info.ok.unwrap_or(true) {
        if let Some(code) = info.exit_code {
            out.push_str(&format!(" (exit={code})"));
        } else if let Some(ec) = info.error_code.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!(" ({ec})"));
        }
    }
    out
}

pub fn tool_card_text(info: &ToolResultInfo, loc: Locale) -> String {
    let out = normalized_tool_summary(info, loc);
    let title = render_tool_title(info, loc);
    let body = summary_without_redundant_title(&title, &out);
    let mut merged = title;
    if !body.is_empty() {
        merged.push_str("\n\n");
        merged.push_str(&body);
    }
    // SSE 的 `summary` 仅为短摘要；`output` 含「命令：」「路径：」「从→到：」「搜索：」等结构化首行与正文，展开详情时一并展示。
    const TOOLS_APPEND_RAW_OUTPUT: &[&str] = &[
        "run_command",
        "create_file",
        "modify_file",
        "copy_file",
        "move_file",
        "search_in_files",
    ];
    if TOOLS_APPEND_RAW_OUTPUT.contains(&info.name.trim()) {
        let raw = info.output.trim();
        if !raw.is_empty() {
            merged.push_str("\n\n");
            merged.push_str(raw);
        }
    }
    if let Some(block) = build_tool_failure_block(info, loc, &out) {
        merged.push_str("\n\n");
        merged.push_str(&block);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::{tool_card_compact_text, tool_card_text};
    use crate::i18n::Locale;
    use crate::sse_dispatch::ToolResultInfo;

    fn mk(summary: &str) -> ToolResultInfo {
        ToolResultInfo {
            name: "run_command".to_string(),
            goal_id: None,
            tool_call_id: None,
            result_version: 1,
            summary: Some(summary.to_string()),
            output: String::new(),
            ok: Some(true),
            exit_code: Some(0),
            error_code: None,
            failure_category: None,
            structured_preview: None,
        }
    }

    #[test]
    fn rewrite_raw_success_summary_to_readable_text() {
        let s = "✅ run_command 成功: 退出码：0 标准输出： build CMakeLists.txt main.cpp";
        let out = tool_card_text(&mk(s), Locale::ZhHans);
        assert!(out.starts_with("命令执行"));
        assert!(!out.starts_with("命令执行完成"));
        assert!(!out.contains("run_command 已完成"));
        assert!(out.contains("输出：build CMakeLists.txt main.cpp"));
        assert!(!out.contains("完成了什么"));
        assert!(!out.contains("已成功完成"));
        assert!(!out.contains("✅"));
    }

    #[test]
    fn failed_tool_uses_standardized_sections() {
        let mut info = mk("❌ run_command 失败: 退出码：1 标准错误： permission denied");
        info.ok = Some(false);
        info.exit_code = Some(1);
        info.error_code = Some("command_failed".to_string());
        let out = tool_card_text(&info, Locale::ZhHans);
        assert!(out.contains("发生了什么"));
        assert!(out.contains("影响范围"));
        assert!(out.contains("建议下一步"));
    }

    #[test]
    fn compact_text_stays_single_line_and_no_template_headers() {
        let s = "✅ run_command 成功: 退出码：0 标准输出： build CMakeLists.txt";
        let out = tool_card_compact_text(&mk(s), Locale::ZhHans);
        assert!(!out.contains("完成了什么"));
        assert!(!out.contains('\n'));
        assert!(!out.contains("run_command 已完成"));
    }

    #[test]
    fn compact_run_command_prefers_invocation_from_output() {
        let mut info = mk("cargo check");
        info.name = "run_command".to_string();
        info.output = "命令：cargo check --workspace\n退出码：0\n(无输出)\n".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("命令执行"));
        assert!(out.contains("cargo check --workspace"), "compact={out}");
        assert!(
            !out.contains("命令 ｜ cargo"),
            "不应重复「命令」标签: compact={out}"
        );
    }

    #[test]
    fn compact_read_dir_uses_short_human_signal() {
        let mut info = mk("✅ read_dir 成功: 目录： . 总计遍历： 0，展示： 0");
        info.name = "read_dir".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("读取目录"));
        assert!(!out.contains("读取目录完成"));
        assert!(out.contains(". ｜ 0 项"));
        assert!(
            !out.contains("目录 . ｜"),
            "不应重复「目录」标签: compact={out}"
        );
    }

    #[test]
    fn compact_read_file_uses_path_and_line_count() {
        let mut info = mk("✅ read_file 成功: 路径： src/main.cpp 行数： 128");
        info.name = "read_file".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("读取文件"));
        assert!(!out.contains("读取文件完成"));
        assert!(out.contains("src/main.cpp ｜ 128 行"));
    }

    #[test]
    fn compact_read_file_parses_english_summary_line() {
        let mut info = mk("read file: src/lib.rs [1-10]");
        info.name = "read_file".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("读取文件"));
        assert!(
            out.contains("src/lib.rs"),
            "应展示路径而非占位「文件」: compact={out}"
        );
        assert!(!out.contains("读取文件 ｜ 文件 ｜"));
    }

    #[test]
    fn compact_copy_file_shows_from_to_without_extra_label() {
        let mut info = mk("✅ copy_file 成功");
        info.name = "copy_file".to_string();
        info.output = "从→到：a.txt → b.txt\n已复制（12 字节）".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("a.txt → b.txt"), "compact 应含源→目标: {out}");
        assert!(!out.contains("从→到 ｜"), "不应再叠「从→到」标签: {out}");
    }

    #[test]
    fn compact_read_file_prefers_json_header_in_output() {
        let hdr = r#"{"kind":"crabmate_tool_output","tool":"read_file","version":1,"path":"README.md","start_line":1,"end_line_shown":20,"line_count_returned":20,"total_lines":200,"truncated_by_max_lines":false,"has_more":true,"file_empty":false}"#;
        let mut info = mk("");
        info.name = "read_file".to_string();
        info.summary = Some("read file: README.md".to_string());
        info.output = format!("{hdr}\n文件: README.md\n总行数: 200\n...\n");
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("README.md ｜ 200 行"), "compact={out}");
    }

    #[test]
    fn compact_search_prefers_structured_output_with_scope_and_hits() {
        let hdr = r#"{"kind":"crabmate_tool_output","tool":"search_in_files","version":1,"pattern":"TODO","root":".","match_count":7,"files_visited":20,"max_results":200,"truncated":false}"#;
        let mut info = mk("✅ search_in_files 成功");
        info.name = "search_in_files".to_string();
        info.output =
            format!("{hdr}\n搜索：\"TODO\"\n范围：.\n匹配结果（最多 200 条，实际 7 条）：\n\n");
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("全文检索"));
        assert!(!out.contains("全文检索完成"));
        assert!(
            out.contains("搜索 ｜ TODO · . · 命中 7 处"),
            "compact={out}"
        );
    }

    #[test]
    fn compact_search_legacy_summary_fallback_keyword_and_hits() {
        let mut info = mk("✅ search_in_files 成功: 关键词： TODO 命中： 7");
        info.name = "search_in_files".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("全文检索"));
        assert!(out.contains("关键词 TODO ｜ 命中 7 处"));
    }
}
