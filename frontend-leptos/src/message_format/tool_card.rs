//! 工具结果卡片的展示用单行/多行摘要（与 SSE `ToolResultInfo` 对齐）。

use crate::i18n::Locale;
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
    let normalized = sum
        .replace(" 退出码：", "\n退出码：")
        .replace(" 标准输出：", "\n标准输出：")
        .replace(" 标准错误：", "\n标准错误：");
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
    if let Some((left, right)) = first.split_once(" 成功:") {
        let tool = left.trim();
        let rest = right.trim();
        title = match loc {
            Locale::ZhHans => format!("{} 已完成", tool_name_human(tool, loc)),
            Locale::En => format!("{tool} completed"),
        };
        if !rest.is_empty() {
            lines.insert(0, rest.to_string());
        }
    } else if let Some((left, right)) = first.split_once(" 失败:") {
        let tool = left.trim();
        let rest = right.trim();
        title = match loc {
            Locale::ZhHans => format!("{} 执行失败", tool_name_human(tool, loc)),
            Locale::En => format!("{tool} failed"),
        };
        if !rest.is_empty() {
            lines.insert(0, rest.to_string());
        }
    }

    let mut extras: Vec<String> = Vec::new();
    for line in lines {
        if line == "退出码：0" {
            continue;
        }
        if let Some(v) = line.strip_prefix("标准输出：") {
            let v = v.trim();
            if !v.is_empty() {
                let label = match loc {
                    Locale::ZhHans => "输出",
                    Locale::En => "Output",
                };
                extras.push(format!("{label}：{v}"));
            }
            continue;
        }
        if let Some(v) = line.strip_prefix("标准错误：") {
            let v = v.trim();
            if !v.is_empty() {
                let label = match loc {
                    Locale::ZhHans => "错误输出",
                    Locale::En => "Stderr",
                };
                extras.push(format!("{label}：{v}"));
            }
            continue;
        }
        if line.starts_with("退出码：") {
            extras.push(line);
            continue;
        }
        extras.push(line);
    }

    if extras.is_empty() {
        return title;
    }
    format!("{title}\n\n{}", extras.join("\n"))
}

fn tool_name_human(name: &str, loc: Locale) -> String {
    match (loc, name) {
        (Locale::ZhHans, "run_command") => "命令执行".to_string(),
        (Locale::ZhHans, "read_file") => "读取文件".to_string(),
        (Locale::ZhHans, "read_dir") => "读取目录".to_string(),
        (Locale::ZhHans, "search_in_files") => "全文检索".to_string(),
        (Locale::ZhHans, "list_files") => "列出文件".to_string(),
        (Locale::En, "run_command") => "Command run".to_string(),
        (Locale::En, "read_file") => "Read file".to_string(),
        (Locale::En, "search_in_files") => "Search files".to_string(),
        (Locale::En, "list_files") => "List files".to_string(),
        _ => name.to_string(),
    }
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
            .split("目录：")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .unwrap_or(".");
        let shown = joined
            .split("展示：")
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
        return Some(match loc {
            Locale::ZhHans => join_compact_parts(&format!("目录 {dir}"), &format!("{shown} 项")),
            Locale::En => join_compact_parts(&format!("dir {dir}"), &format!("{shown} entries")),
        });
    }
    if info.name.trim() == "read_file" {
        let joined = lines.join(" ");
        let path = joined
            .split("路径：")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .or_else(|| {
                joined
                    .split("path:")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .unwrap_or("文件");
        let line_count = joined
            .split("行数：")
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
            });
        return Some(match (loc, line_count) {
            (Locale::ZhHans, Some(n)) => join_compact_parts(path, &format!("{n} 行")),
            (Locale::ZhHans, None) => path.to_string(),
            (Locale::En, Some(n)) => join_compact_parts(path, &format!("{n} lines")),
            (Locale::En, None) => path.to_string(),
        });
    }
    if info.name.trim() == "search_in_files" {
        let joined = lines.join(" ");
        let keyword = joined
            .split("关键词：")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .or_else(|| {
                joined
                    .split("pattern:")
                    .nth(1)
                    .and_then(|rest| rest.split_whitespace().next())
            })
            .unwrap_or("关键词");
        let hit_count = joined
            .split("命中：")
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
            });
        return Some(match (loc, hit_count) {
            (Locale::ZhHans, Some(n)) => {
                join_compact_parts(&format!("关键词 {keyword}"), &format!("命中 {n} 处"))
            }
            (Locale::ZhHans, None) => format!("关键词 {keyword}"),
            (Locale::En, Some(n)) => {
                join_compact_parts(&format!("keyword {keyword}"), &format!("{n} hits"))
            }
            (Locale::En, None) => format!("keyword {keyword}"),
        });
    }
    lines
        .iter()
        .find(|l| l.contains("输出：") || l.contains("错误输出：") || l.contains("目录："))
        .map(|s| s.to_string())
        .or_else(|| lines.first().map(|s| (*s).to_string()))
}

fn render_tool_title(info: &ToolResultInfo, loc: Locale) -> String {
    let human = tool_name_human(info.name.trim(), loc);
    let ok = info.ok.unwrap_or(true);
    if ok {
        match loc {
            Locale::ZhHans => format!("{human}完成"),
            Locale::En => format!("{human} done"),
        }
    } else {
        match loc {
            Locale::ZhHans => format!("{human}失败"),
            Locale::En => format!("{human} failed"),
        }
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
        match loc {
            Locale::ZhHans => "建议：缩小命令范围后重试，或提高超时阈值。",
            Locale::En => "Suggestion: retry with narrower scope or increase timeout.",
        }
    } else if code.contains("invalid") || code.contains("arg") {
        match loc {
            Locale::ZhHans => "建议：检查命令参数格式与路径是否正确。",
            Locale::En => "Suggestion: verify command args format and paths.",
        }
    } else {
        match loc {
            Locale::ZhHans => "建议：检查错误输出并按需重试。",
            Locale::En => "Suggestion: inspect stderr and retry if needed.",
        }
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
        match loc {
            Locale::ZhHans => format!("工具返回错误码：{code}"),
            Locale::En => format!("Tool returned error code: {code}"),
        }
    } else {
        match loc {
            Locale::ZhHans => "工具执行失败。".to_string(),
            Locale::En => "Tool execution failed.".to_string(),
        }
    };
    let impact = if let Some(ec) = info.exit_code {
        match loc {
            Locale::ZhHans => format!("当前步骤中断（退出码：{ec}），本轮后续动作可能被跳过。"),
            Locale::En => {
                format!("This step stopped (exit code: {ec}); follow-up actions may be skipped.")
            }
        }
    } else {
        match loc {
            Locale::ZhHans => "当前步骤中断，本轮后续动作可能被跳过。".to_string(),
            Locale::En => "This step stopped; follow-up actions may be skipped.".to_string(),
        }
    };
    let suggestion = build_tool_failure_suggestion(info, loc).unwrap_or_else(|| match loc {
        Locale::ZhHans => "检查错误输出并按需重试。".to_string(),
        Locale::En => "Inspect stderr and retry if needed.".to_string(),
    });
    Some(match loc {
        Locale::ZhHans => {
            format!("发生了什么\n{happened}\n\n影响范围\n{impact}\n\n建议下一步\n{suggestion}")
        }
        Locale::En => {
            format!("What happened\n{happened}\n\nImpact\n{impact}\n\nNext step\n{suggestion}")
        }
    })
}

fn build_tool_success_block(info: &ToolResultInfo, loc: Locale, body: &str) -> Option<String> {
    if !info.ok.unwrap_or(true) {
        return None;
    }
    let done = match loc {
        Locale::ZhHans => format!("{} 已成功完成。", tool_name_human(info.name.trim(), loc)),
        Locale::En => format!(
            "{} completed successfully.",
            tool_name_human(info.name.trim(), loc)
        ),
    };
    let output = if body.trim().is_empty() {
        match loc {
            Locale::ZhHans => "未返回可展示的输出摘要。".to_string(),
            Locale::En => "No displayable output summary returned.".to_string(),
        }
    } else {
        body.trim().to_string()
    };
    let next = match info.name.trim() {
        "run_command" => match loc {
            Locale::ZhHans => "可继续：检查输出后执行下一条命令或进入验证。",
            Locale::En => "Next: inspect output, then run next command or verify.",
        },
        "read_file" => match loc {
            Locale::ZhHans => "可继续：基于读取结果定位修改点或继续检索相关文件。",
            Locale::En => "Next: locate edit points or continue searching related files.",
        },
        _ => match loc {
            Locale::ZhHans => "可继续：基于当前结果继续下一步操作。",
            Locale::En => "Next: continue with the next step based on this result.",
        },
    };
    Some(match loc {
        Locale::ZhHans => {
            format!("完成了什么\n{done}\n\n产出是什么\n{output}\n\n可继续做什么\n{next}")
        }
        Locale::En => {
            format!("What was done\n{done}\n\nWhat was produced\n{output}\n\nWhat next\n{next}")
        }
    })
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
    if let Some(c) = candidate.as_deref()
        && !c.is_empty()
        && c != title
        && !matches!(c, "完成了什么" | "产出是什么" | "可继续做什么")
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
