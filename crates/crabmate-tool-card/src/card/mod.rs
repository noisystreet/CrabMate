//! 工具结果卡片的展示用单行/多行摘要（与 SSE `ToolCardInput` 对齐）。

use crate::ToolCardInput;
use crate::locale::{self, ToolCardLocale};

use crate::plain::collapse_duplicate_summary_lines;
use crate::strip_ansi::strip_ansi_codes;

use serde_json::Value;

mod compact_key;
mod title_signal_dedup;

use title_signal_dedup::{
    tool_compact_signal_paren_suffix_after_redundant_head, tool_compact_signal_redundant_with_title,
};

const COMPACT_SEPARATOR: &str = " ｜ ";

fn strip_leading_workspace_write_json_header(raw: &str) -> String {
    let first = raw.lines().next().map(str::trim).unwrap_or("");
    if first.is_empty() || !first.starts_with('{') {
        return raw.to_string();
    }
    let Ok(v) = serde_json::from_str::<Value>(first) else {
        return raw.to_string();
    };
    if v.get("kind").and_then(|k| k.as_str()) != Some("crabmate_tool_output") {
        return raw.to_string();
    }
    if v.get("preview").and_then(|p| p.as_str()) != Some("workspace_write_diff") {
        return raw.to_string();
    }
    raw.lines().skip(1).collect::<Vec<&str>>().join("\n")
}

fn structured_preview_write_diff_root(sp: &Value) -> Option<&Value> {
    if let Some(h) = sp.get("tool_output_header") {
        return (h.get("preview").and_then(|p| p.as_str()) == Some("workspace_write_diff"))
            .then_some(h);
    }
    (sp.get("preview").and_then(|p| p.as_str()) == Some("workspace_write_diff")).then_some(sp)
}

fn workspace_write_diff_section(info: &ToolCardInput, loc: ToolCardLocale) -> Option<String> {
    let sp = info.structured_preview.as_ref()?;
    let root = structured_preview_write_diff_root(sp)?;
    let files = root.get("files")?.as_array()?;
    if files.is_empty() {
        return None;
    }
    let mut blocks: Vec<String> = Vec::new();
    blocks.push(locale::tool_workspace_write_diff_heading(loc).to_string());
    let mut any = false;
    for f in files {
        let path = f.get("path").and_then(|p| p.as_str()).unwrap_or("?");
        let udiff = f.get("unified_diff").and_then(|u| u.as_str()).unwrap_or("");
        if udiff.is_empty() {
            continue;
        }
        any = true;
        blocks.push(format!("`{path}`\n```diff\n{udiff}\n```"));
    }
    if !any {
        return None;
    }
    if root
        .get("preview_truncated")
        .and_then(|b| b.as_bool())
        .unwrap_or(false)
    {
        blocks.push(locale::tool_workspace_write_diff_truncated_note(loc).to_string());
    }
    Some(blocks.join("\n\n"))
}

fn should_strip_write_preview_header(tool_name: &str, raw: &str) -> bool {
    const NAMES: &[&str] = &[
        "create_file",
        "modify_file",
        "copy_file",
        "search_replace",
        "delete_file",
        "append_file",
        "apply_patch",
    ];
    NAMES.contains(&tool_name)
        && raw
            .lines()
            .next()
            .is_some_and(|l| l.trim_start().starts_with('{'))
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

fn legacy_tool_title_from_first_line(
    first_raw: String,
    loc: ToolCardLocale,
    remainder: &mut Vec<String>,
) -> String {
    let first = strip_tool_status_prefix(&first_raw);
    let mut title = first.clone();
    if let Some((left, right)) = first.split_once(locale::tool_cmd_success_sep(loc)) {
        let tool = left.trim();
        let rest = right.trim();
        title = locale::tool_rewrite_title_done(loc, &locale::tool_human_name(loc, tool));
        if !rest.is_empty() {
            remainder.insert(0, rest.to_string());
        }
    } else if let Some((left, right)) = first.split_once(locale::tool_cmd_fail_sep(loc)) {
        let tool = left.trim();
        let rest = right.trim();
        title = locale::tool_rewrite_title_failed_run(loc, &locale::tool_human_name(loc, tool));
        if !rest.is_empty() {
            remainder.insert(0, rest.to_string());
        }
    }
    title
}

fn collect_legacy_tool_extra_lines(lines: Vec<String>, loc: ToolCardLocale) -> Vec<String> {
    let mut extras: Vec<String> = Vec::new();
    for line in lines {
        if line == locale::tool_exit_line_zero(loc)
            || line == locale::tool_exit_line_zero(ToolCardLocale::ZhHans)
        {
            continue;
        }
        if let Some(v) = line
            .strip_prefix(locale::tool_line_stdout_prefix(loc))
            .or_else(|| line.strip_prefix(locale::tool_line_stdout_prefix(ToolCardLocale::ZhHans)))
        {
            let v = v.trim();
            if !v.is_empty() {
                let label = locale::tool_summary_label_stdout(loc);
                extras.push(format!("{label}：{v}"));
            }
            continue;
        }
        if let Some(v) = line
            .strip_prefix(locale::tool_line_stderr_prefix(loc))
            .or_else(|| line.strip_prefix(locale::tool_line_stderr_prefix(ToolCardLocale::ZhHans)))
        {
            let v = v.trim();
            if !v.is_empty() {
                let label = locale::tool_summary_label_stderr(loc);
                extras.push(format!("{label}：{v}"));
            }
            continue;
        }
        if line.starts_with(locale::tool_line_exit_prefix(loc))
            || line.starts_with(locale::tool_line_exit_prefix(ToolCardLocale::ZhHans))
        {
            extras.push(line.to_string());
            continue;
        }
        extras.push(line);
    }
    extras
}

fn rewrite_legacy_tool_summary(sum: &str, loc: ToolCardLocale) -> String {
    let normalized = locale::tool_summary_normalize_line_breaks(sum, loc);
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
    let title = legacy_tool_title_from_first_line(first_raw, loc, &mut lines);
    let extras = collect_legacy_tool_extra_lines(lines, loc);

    if extras.is_empty() {
        return title;
    }
    format!("{title}\n\n{}", extras.join("\n"))
}

#[inline]
fn tool_name_human(name: &str, loc: ToolCardLocale) -> String {
    locale::tool_human_name(loc, name)
}

fn render_tool_title(info: &ToolCardInput, loc: ToolCardLocale) -> String {
    let human = tool_name_human(info.name.trim(), loc);
    let ok = info.ok.unwrap_or(true);
    if ok {
        locale::tool_title_completed(loc, &human)
    } else {
        locale::tool_title_failed(loc, &human)
    }
}

fn build_tool_failure_suggestion(info: &ToolCardInput, loc: ToolCardLocale) -> Option<String> {
    if info.ok.unwrap_or(true) {
        return None;
    }
    let code = info
        .error_code
        .as_deref()
        .unwrap_or("")
        .to_ascii_lowercase();
    let hint = if code.contains("timeout") {
        locale::tool_failure_suggest_timeout(loc)
    } else if code.contains("invalid") || code.contains("arg") {
        locale::tool_failure_suggest_invalid_args(loc)
    } else {
        locale::tool_failure_suggest_generic(loc)
    };
    Some(hint.to_string())
}

fn build_tool_failure_block(
    info: &ToolCardInput,
    loc: ToolCardLocale,
    body: &str,
) -> Option<String> {
    if info.ok.unwrap_or(true) {
        return None;
    }
    let happened = if !body.trim().is_empty() {
        body.trim().to_string()
    } else if let Some(code) = info.error_code.as_deref().filter(|s| !s.is_empty()) {
        locale::tool_failure_returned_code(loc, code)
    } else {
        locale::tool_failure_no_detail(loc).to_string()
    };
    let impact = if let Some(ec) = info.exit_code {
        locale::tool_failure_impact_exit(loc, ec)
    } else {
        locale::tool_failure_impact_no_exit(loc).to_string()
    };
    let suggestion = build_tool_failure_suggestion(info, loc)
        .unwrap_or_else(|| locale::tool_failure_suggest_fallback(loc).to_string());
    Some(locale::format_error_three_part(
        loc,
        happened.as_str(),
        impact.as_str(),
        suggestion.as_str(),
    ))
}

/// 去掉流式占位曾拼接的「· 工具执行中…」类后缀，避免终态工具卡仍带加载文案。
fn strip_placeholder_tool_running_suffix(s: &str) -> String {
    const SUFFIXES: &[&str] = &[
        " · 工具执行中…",
        " · 工具执行中",
        " · Running tools…",
        " · Running tools",
    ];
    fn strip_line(line: &str) -> String {
        let mut t = line.trim_end();
        loop {
            let mut stripped = None;
            for sfx in SUFFIXES {
                if let Some(rest) = t.strip_suffix(sfx) {
                    stripped = Some(rest.trim_end());
                    break;
                }
            }
            match stripped {
                Some(r) => t = r,
                None => break,
            }
        }
        t.to_string()
    }
    if !s.contains('\n') {
        return strip_line(s);
    }
    s.lines()
        .map(strip_line)
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
}

fn normalized_tool_summary(info: &ToolCardInput, loc: ToolCardLocale) -> String {
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
    let rest: Vec<String> = lines
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != first.as_str())
        .filter_map(|l| {
            if tool_compact_signal_redundant_with_title(first.as_str(), l) {
                tool_compact_signal_paren_suffix_after_redundant_head(first.as_str(), l)
            } else {
                Some(l.to_string())
            }
        })
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
    strip_placeholder_tool_running_suffix(&out)
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

pub fn tool_card_compact_text(info: &ToolCardInput, loc: ToolCardLocale) -> String {
    let title = render_tool_title(info, loc);
    let summary = normalized_tool_summary(info, loc);
    let candidate = compact_key::compact_key_signal(info, &summary, loc);
    let mut out = title.clone();
    let skip_compact = locale::tool_card_compact_skip_headings(loc);
    if let Some(c) = candidate.as_deref()
        && !c.is_empty()
        && c != title
        && !skip_compact.contains(&c)
    {
        if tool_compact_signal_redundant_with_title(&title, c) {
            if let Some(tail) = tool_compact_signal_paren_suffix_after_redundant_head(&title, c) {
                out.push_str(COMPACT_SEPARATOR);
                out.push_str(&tail);
            }
        } else {
            out.push_str(COMPACT_SEPARATOR);
            out.push_str(c);
        }
    }
    if !info.ok.unwrap_or(true) {
        if let Some(code) = info.exit_code {
            out.push_str(&format!(" (exit={code})"));
        } else if let Some(ec) = info.error_code.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(&format!(" ({ec})"));
        }
    }
    strip_placeholder_tool_running_suffix(&out)
}

fn terminal_session_shell_line_from_summary(summary: Option<&str>) -> Option<String> {
    let s = summary?.trim();
    const PREFIX: &str = "terminal_session exec ";
    let rest = s.strip_prefix(PREFIX)?.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

/// `terminal_session` 成功：展开区 `$ <cmd>` + 捕获输出；无可展示内容则 `None` 走通用路径。
fn tool_card_text_terminal_session_early(info: &ToolCardInput) -> Option<String> {
    if info.name.trim() != "terminal_session" || !info.ok.unwrap_or(true) {
        return None;
    }
    let raw = strip_ansi_codes(info.output.trim());
    if let Some(cmd) = terminal_session_shell_line_from_summary(info.summary.as_deref()) {
        return Some(if raw.is_empty() {
            format!("$ {cmd}\n")
        } else {
            format!("$ {cmd}\n\n{raw}")
        });
    }
    (!raw.is_empty()).then_some(raw)
}

/// 不拼接 `info.output` 全文的工具：多为首行 **`crabmate_tool_output`** 的结构化结果、
/// 目录/语义检索体量过大、日程 CRUD 短文、问卷 JSON 等（展开区仍以摘要/结构化预览为主）。
const TOOLS_SKIP_APPEND_RAW_OUTPUT: &[&str] = &[
    "read_file",
    "read_dir",
    "list_tree",
    "glob_files",
    "file_exists",
    "read_binary_meta",
    "hash_file",
    "extract_in_file",
    "codebase_semantic_search",
    "present_clarification_questionnaire",
    "add_reminder",
    "list_reminders",
    "complete_reminder",
    "delete_reminder",
    "update_reminder",
    "add_event",
    "list_events",
    "delete_event",
    "update_event",
    "long_term_remember",
    "long_term_forget",
    "long_term_memory_list",
];

/// 详情区是否拼接 `info.output` 全文（供展开抽屉 `<pre>` 展示；服务端仍会做长度截断）。
///
/// 默认 **拼接**（与 `run_command` 一致），仅 [`TOOLS_SKIP_APPEND_RAW_OUTPUT`] 中的工具例外，
/// 以免列表类/结构化工具在 UI 重复巨量正文。
#[inline]
fn tool_should_append_raw_output(name_trim: &str) -> bool {
    !TOOLS_SKIP_APPEND_RAW_OUTPUT.contains(&name_trim)
}

/// 从 `run_command` 正文首行 `命令：…` 或（回退）单行摘要取「调用串」，供详情卡标题 `$ …` 与去重用。
fn run_command_invocation_for_display(info: &ToolCardInput, summary_norm: &str) -> Option<String> {
    if info.name.trim() != "run_command" {
        return None;
    }
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
    let s = summary_norm.trim();
    if s.is_empty() || s.contains('\n') {
        return None;
    }
    let t = strip_tool_status_prefix(s);
    if t.contains("成功") || t.contains("失败") {
        return None;
    }
    (!t.is_empty()).then_some(t)
}

/// 去掉与调用串完全相同的摘要行，避免标题 `$ …` 下再重复一行。
fn strip_summary_lines_matching_invocation(out: &str, inv: &str) -> String {
    let inv = inv.trim();
    let kept: Vec<&str> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != inv)
        .collect();
    kept.join("\n\n")
}

/// 若正文首行 `命令：<inv>` 与标题已展示的 inv 相同，则去掉该行（保留退出码与 stdio）。
fn strip_run_command_dup_cmd_header(raw: &str, inv: &str) -> String {
    let inv = inv.trim();
    let mut it = raw.lines();
    let Some(first) = it.next() else {
        return raw.to_string();
    };
    if first.trim().strip_prefix("命令：").map(str::trim) == Some(inv) {
        let rest: Vec<&str> = it.collect();
        if rest.is_empty() {
            return String::new();
        }
        return rest.join("\n");
    }
    raw.to_string()
}

fn append_workspace_diff_and_whitelist_raw(
    merged: &mut String,
    info: &ToolCardInput,
    loc: ToolCardLocale,
    raw_trimmed: &str,
    name_trim: &str,
    whitelist_tool: bool,
    run_command_shell_inv: Option<&str>,
) {
    if let Some(block) = workspace_write_diff_section(info, loc) {
        merged.push_str("\n\n");
        merged.push_str(&block);
    }
    if !whitelist_tool || raw_trimmed.is_empty() {
        return;
    }
    merged.push_str("\n\n");
    let mut raw_show = if should_strip_write_preview_header(name_trim, raw_trimmed) {
        strip_leading_workspace_write_json_header(raw_trimmed)
    } else {
        raw_trimmed.to_string()
    };
    if let Some(inv) = run_command_shell_inv {
        raw_show = strip_run_command_dup_cmd_header(&raw_show, inv);
    }
    if name_trim == "terminal_session" {
        merged.push_str(&strip_ansi_codes(&raw_show));
    } else {
        merged.push_str(&raw_show);
    }
}

fn append_failure_block_and_full_output_on_fail(
    merged: &mut String,
    info: &ToolCardInput,
    loc: ToolCardLocale,
    summary_normalized: &str,
    raw_trimmed: &str,
    name_trim: &str,
    whitelist_tool: bool,
) {
    if let Some(block) = build_tool_failure_block(info, loc, summary_normalized) {
        merged.push_str("\n\n");
        merged.push_str(&block);
    }
    if info.ok.unwrap_or(true) || raw_trimmed.is_empty() || whitelist_tool {
        return;
    }
    merged.push_str("\n\n");
    merged.push_str(locale::tool_detail_full_output_heading(loc));
    merged.push('\n');
    if name_trim == "terminal_session" {
        merged.push_str(&strip_ansi_codes(raw_trimmed));
    } else {
        merged.push_str(raw_trimmed);
    }
}

pub fn tool_card_text(info: &ToolCardInput, loc: ToolCardLocale) -> String {
    if let Some(early) = tool_card_text_terminal_session_early(info) {
        return early;
    }

    let mut title = render_tool_title(info, loc);
    let mut out = normalized_tool_summary(info, loc);
    let mut run_shell_inv: Option<String> = None;
    if info.name.trim() == "run_command"
        && info.ok.unwrap_or(true)
        && let Some(inv) = run_command_invocation_for_display(info, &out)
    {
        run_shell_inv = Some(inv.clone());
        title = format!("$ {inv}");
        out = strip_summary_lines_matching_invocation(&out, &inv);
    }
    let body = summary_without_redundant_title(&title, &out);
    let body = if tool_compact_signal_redundant_with_title(&title, body.trim()) {
        String::new()
    } else {
        body
    };
    let mut merged = title;
    if !body.is_empty() {
        merged.push_str("\n\n");
        merged.push_str(&body);
    }
    let raw_trimmed = info.output.trim();
    let name_trim = info.name.trim();
    let whitelist_tool = tool_should_append_raw_output(name_trim);
    append_workspace_diff_and_whitelist_raw(
        &mut merged,
        info,
        loc,
        raw_trimmed,
        name_trim,
        whitelist_tool,
        run_shell_inv.as_deref(),
    );
    append_failure_block_and_full_output_on_fail(
        &mut merged,
        info,
        loc,
        &out,
        raw_trimmed,
        name_trim,
        whitelist_tool,
    );
    merged
}

#[cfg(test)]
mod tests;
