//! 工具结果卡片的展示用单行/多行摘要（与 SSE `ToolResultInfo` 对齐）。

use crate::i18n::{self, Locale};
use crate::sse_dispatch::ToolResultInfo;

use super::plain::collapse_duplicate_summary_lines;

const COMPACT_SEPARATOR: &str = " ｜ ";

fn join_compact_parts(left: &str, right: &str) -> String {
    format!("{left}{COMPACT_SEPARATOR}{right}")
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
        return Some(join_compact_parts(
            &format!("{} {dir}", i18n::tool_read_dir_compact_dir_word(loc)),
            &i18n::tool_read_dir_compact_entries(loc, shown),
        ));
    }
    if info.name.trim() == "read_file" {
        let joined = lines.join(" ");
        let path = joined
            .split(i18n::tool_read_file_label_path(loc))
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .or_else(|| {
                joined
                    .split(i18n::tool_read_file_label_path(Locale::ZhHans))
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .or_else(|| {
                joined
                    .split("path:")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .unwrap_or(i18n::tool_read_file_default_path(loc));
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
            Some(n) => join_compact_parts(path, &i18n::tool_read_file_lines_suffix(loc, n)),
            None => path.to_string(),
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

fn build_tool_success_block(info: &ToolResultInfo, loc: Locale, body: &str) -> Option<String> {
    if !info.ok.unwrap_or(true) {
        return None;
    }
    let done = i18n::tool_success_done_line(loc, &tool_name_human(info.name.trim(), loc));
    let output = if body.trim().is_empty() {
        i18n::tool_success_no_output_summary(loc).to_string()
    } else {
        body.trim().to_string()
    };
    let next = match info.name.trim() {
        "run_command" => i18n::tool_success_next_run_command(loc).to_string(),
        "read_file" => i18n::tool_success_next_read_file(loc).to_string(),
        _ => i18n::tool_success_next_generic(loc).to_string(),
    };
    Some(i18n::format_success_three_part(
        loc,
        done.as_str(),
        output.as_str(),
        next.as_str(),
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
    let mut merged = render_tool_title(info, loc);
    if !out.trim().is_empty() {
        merged.push_str("\n\n");
        merged.push_str(out.trim());
    }
    if let Some(block) = build_tool_success_block(info, loc, &out) {
        merged.push_str("\n\n");
        merged.push_str(&block);
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
            result_version: 1,
            summary: Some(summary.to_string()),
            output: String::new(),
            ok: Some(true),
            exit_code: Some(0),
            error_code: None,
            failure_category: None,
        }
    }

    #[test]
    fn rewrite_raw_success_summary_to_readable_text() {
        let s = "✅ run_command 成功: 退出码：0 标准输出： build CMakeLists.txt main.cpp";
        let out = tool_card_text(&mk(s), Locale::ZhHans);
        assert!(out.starts_with("命令执行完成"));
        assert!(!out.contains("run_command 已完成"));
        assert!(out.contains("输出：build CMakeLists.txt main.cpp"));
        assert!(out.contains("完成了什么"));
        assert!(out.contains("产出是什么"));
        assert!(out.contains("可继续做什么"));
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
    fn compact_read_dir_uses_short_human_signal() {
        let mut info = mk("✅ read_dir 成功: 目录： . 总计遍历： 0，展示： 0");
        info.name = "read_dir".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("读取目录完成"));
        assert!(out.contains("目录 . ｜ 0 项"));
    }

    #[test]
    fn compact_read_file_uses_path_and_line_count() {
        let mut info = mk("✅ read_file 成功: 路径： src/main.cpp 行数： 128");
        info.name = "read_file".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("读取文件完成"));
        assert!(out.contains("src/main.cpp ｜ 128 行"));
    }

    #[test]
    fn compact_search_uses_keyword_and_hit_count() {
        let mut info = mk("✅ search_in_files 成功: 关键词： TODO 命中： 7");
        info.name = "search_in_files".to_string();
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(out.contains("全文检索完成"));
        assert!(out.contains("关键词 TODO ｜ 命中 7 处"));
    }
}
