//! 工具卡片「紧凑副标题」：从 `tool_output` / 摘要行提取单行信号。

use crate::ToolCardInput;
use crate::locale::{self, ToolCardLocale};

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

fn compact_path_prefix_from_output(info: &ToolCardInput) -> Option<String> {
    let v = first_nonempty_line_with_prefix(&info.output, "路径：")?;
    Some(v.to_string())
}

fn compact_from_to_from_output(info: &ToolCardInput) -> Option<String> {
    for line in info.output.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("从→到：") {
            let rest = rest.trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
        break;
    }
    None
}

fn compact_search_from_tool_output(output: &str, loc: ToolCardLocale) -> Option<String> {
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
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(lines[0])
        && v.get("kind").and_then(|x| x.as_str()) == Some("crabmate_tool_output")
        && v.get("tool").and_then(|x| x.as_str()) == Some("search_in_files")
    {
        match_count = v.get("match_count").and_then(|x| x.as_u64());
        idx = 1;
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
        right.push_str(&locale::tool_search_compact_hits_suffix(loc, n as usize));
    }
    Some(join_compact_parts(
        locale::tool_search_compact_header_label(loc),
        &right,
    ))
}

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

fn compact_read_file_from_output(output: &str, loc: ToolCardLocale) -> Option<String> {
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
        Some(n) => join_compact_parts(path, &locale::tool_read_file_lines_suffix(loc, n)),
        None => path.to_string(),
    })
}

fn compact_from_named_output(info: &ToolCardInput, loc: ToolCardLocale) -> Option<String> {
    match info.name.trim() {
        "run_command" => {
            for line in info.output.lines().map(str::trim) {
                if line.is_empty() {
                    continue;
                }
                if let Some(inv) = line.strip_prefix("命令：") {
                    let inv = inv.trim();
                    if !inv.is_empty() {
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
    None
}

fn compact_read_dir_from_summary_lines(lines: &[&str], loc: ToolCardLocale) -> Option<String> {
    let joined = lines.join(" ");
    let dir = joined
        .split(locale::tool_read_dir_label_dir(loc))
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .or_else(|| {
            joined
                .split(locale::tool_read_dir_label_dir(ToolCardLocale::ZhHans))
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
        })
        .unwrap_or(".");
    let shown = joined
        .split(locale::tool_read_dir_label_shown(loc))
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
    Some(join_compact_parts(
        dir,
        &locale::tool_read_dir_compact_entries(loc, shown),
    ))
}

fn compact_read_file_from_summary_lines(lines: &[&str], loc: ToolCardLocale) -> Option<String> {
    let joined = lines.join(" ");
    let path = path_from_summary_read_file_english(&joined)
        .or_else(|| {
            joined
                .split(locale::tool_read_file_label_path(loc))
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
                .map(str::to_string)
        })
        .or_else(|| {
            joined
                .split(locale::tool_read_file_label_path(ToolCardLocale::ZhHans))
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
        .unwrap_or_else(|| locale::tool_read_file_default_path(loc).to_string());
    let line_count = joined
        .split(locale::tool_read_file_label_lines(loc))
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
                .split(locale::tool_read_file_label_lines(ToolCardLocale::ZhHans))
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
    Some(match line_count {
        Some(n) => join_compact_parts(&path, &locale::tool_read_file_lines_suffix(loc, n)),
        None => path,
    })
}

fn compact_search_from_summary_lines(lines: &[&str], loc: ToolCardLocale) -> Option<String> {
    let joined = lines.join(" ");
    let keyword = joined
        .split(locale::tool_search_label_keyword(loc))
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .or_else(|| {
            joined
                .split(locale::tool_search_label_keyword(ToolCardLocale::ZhHans))
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
        })
        .or_else(|| {
            joined
                .split("pattern:")
                .nth(1)
                .and_then(|rest| rest.split_whitespace().next())
        })
        .unwrap_or(locale::tool_search_default_keyword(loc));
    let hit_count = joined
        .split(locale::tool_search_label_hits(loc))
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
                .split(locale::tool_search_label_hits(ToolCardLocale::ZhHans))
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
    Some(match hit_count {
        Some(n) => join_compact_parts(
            &format!(
                "{} {keyword}",
                locale::tool_search_compact_keyword_word(loc)
            ),
            &locale::tool_search_compact_hits_suffix(loc, n),
        ),
        None => format!(
            "{} {keyword}",
            locale::tool_search_compact_keyword_word(loc)
        ),
    })
}

pub(super) fn compact_key_signal(
    info: &ToolCardInput,
    summary: &str,
    loc: ToolCardLocale,
) -> Option<String> {
    if let Some(s) = compact_from_named_output(info, loc) {
        return Some(s);
    }

    let lines = summary
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }
    let name = info.name.trim();
    if name == "read_dir" {
        return compact_read_dir_from_summary_lines(&lines, loc);
    }
    if name == "read_file" {
        return compact_read_file_from_summary_lines(&lines, loc);
    }
    if name == "search_in_files" {
        return compact_search_from_summary_lines(&lines, loc);
    }
    lines
        .iter()
        .find(|l| locale::summary_line_looks_like_compact_signal(l, loc))
        .map(|s| s.to_string())
        .or_else(|| lines.first().map(|s| (*s).to_string()))
}
