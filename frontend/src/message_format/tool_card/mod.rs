//! 工具结果卡片的展示用单行/多行摘要（与 SSE `ToolResultInfo` 对齐）。

use crate::i18n::{self, Locale};
use crate::sse_dispatch::ToolResultInfo;

use super::plain::collapse_duplicate_summary_lines;
use super::strip_ansi_codes;

use serde_json::Value;

mod compact_key;

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

fn workspace_write_diff_section(info: &ToolResultInfo, loc: Locale) -> Option<String> {
    let sp = info.structured_preview.as_ref()?;
    let root = structured_preview_write_diff_root(sp)?;
    let files = root.get("files")?.as_array()?;
    if files.is_empty() {
        return None;
    }
    let mut blocks: Vec<String> = Vec::new();
    blocks.push(i18n::tool_workspace_write_diff_heading(loc).to_string());
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
        blocks.push(i18n::tool_workspace_write_diff_truncated_note(loc).to_string());
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
    loc: Locale,
    remainder: &mut Vec<String>,
) -> String {
    let first = strip_tool_status_prefix(&first_raw);
    let mut title = first.clone();
    if let Some((left, right)) = first.split_once(i18n::tool_cmd_success_sep(loc)) {
        let tool = left.trim();
        let rest = right.trim();
        title = i18n::tool_rewrite_title_done(loc, &i18n::tool_human_name(loc, tool));
        if !rest.is_empty() {
            remainder.insert(0, rest.to_string());
        }
    } else if let Some((left, right)) = first.split_once(i18n::tool_cmd_fail_sep(loc)) {
        let tool = left.trim();
        let rest = right.trim();
        title = i18n::tool_rewrite_title_failed_run(loc, &i18n::tool_human_name(loc, tool));
        if !rest.is_empty() {
            remainder.insert(0, rest.to_string());
        }
    }
    title
}

fn collect_legacy_tool_extra_lines(lines: Vec<String>, loc: Locale) -> Vec<String> {
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
    extras
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
    let title = legacy_tool_title_from_first_line(first_raw, loc, &mut lines);
    let extras = collect_legacy_tool_extra_lines(lines, loc);

    if extras.is_empty() {
        return title;
    }
    format!("{title}\n\n{}", extras.join("\n"))
}

#[inline]
fn tool_name_human(name: &str, loc: Locale) -> String {
    i18n::tool_human_name(loc, name)
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

pub fn tool_card_compact_text(info: &ToolResultInfo, loc: Locale) -> String {
    let title = render_tool_title(info, loc);
    let summary = normalized_tool_summary(info, loc);
    let candidate = compact_key::compact_key_signal(info, &summary, loc);
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
    strip_placeholder_tool_running_suffix(&out)
}

fn terminal_session_shell_line_from_summary(summary: Option<&str>) -> Option<String> {
    let s = summary?.trim();
    const PREFIX: &str = "terminal_session exec ";
    let rest = s.strip_prefix(PREFIX)?.trim();
    (!rest.is_empty()).then(|| rest.to_string())
}

/// `terminal_session` 成功：展开区 `$ <cmd>` + 捕获输出；无可展示内容则 `None` 走通用路径。
fn tool_card_text_terminal_session_early(info: &ToolResultInfo) -> Option<String> {
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

const TOOLS_APPEND_RAW_OUTPUT: &[&str] = &[
    "run_command",
    "terminal_session",
    "create_file",
    "modify_file",
    "copy_file",
    "move_file",
    "search_replace",
    "delete_file",
    "append_file",
    "apply_patch",
    "search_in_files",
];

/// 从 `run_command` 正文首行 `命令：…` 或（回退）单行摘要取「调用串」，供详情卡标题 `$ …` 与去重用。
fn run_command_invocation_for_display(info: &ToolResultInfo, summary_norm: &str) -> Option<String> {
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
    info: &ToolResultInfo,
    loc: Locale,
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
    info: &ToolResultInfo,
    loc: Locale,
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
    merged.push_str(i18n::tool_detail_full_output_heading(loc));
    merged.push('\n');
    if name_trim == "terminal_session" {
        merged.push_str(&strip_ansi_codes(raw_trimmed));
    } else {
        merged.push_str(raw_trimmed);
    }
}

pub fn tool_card_text(info: &ToolResultInfo, loc: Locale) -> String {
    if let Some(early) = tool_card_text_terminal_session_early(info) {
        return early;
    }

    let mut title = render_tool_title(info, loc);
    let mut out = normalized_tool_summary(info, loc);
    let mut run_shell_inv: Option<String> = None;
    if info.name.trim() == "run_command" && info.ok.unwrap_or(true) {
        if let Some(inv) = run_command_invocation_for_display(info, &out) {
            run_shell_inv = Some(inv.clone());
            title = format!("$ {inv}");
            out = strip_summary_lines_matching_invocation(&out, &inv);
        }
    }
    let body = summary_without_redundant_title(&title, &out);
    let mut merged = title;
    if !body.is_empty() {
        merged.push_str("\n\n");
        merged.push_str(&body);
    }
    let raw_trimmed = info.output.trim();
    let name_trim = info.name.trim();
    let whitelist_tool = TOOLS_APPEND_RAW_OUTPUT.contains(&name_trim);
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
mod tests {
    use super::{tool_card_compact_text, tool_card_text};
    use crate::i18n::Locale;
    use crate::sse_dispatch::ToolResultInfo;
    use serde_json::json;

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
    fn terminal_session_success_detail_shows_command_and_capture() {
        let mut info = mk("✅ terminal_session 成功: terminal_session exec python3");
        info.name = "terminal_session".to_string();
        info.summary =
            Some("terminal_session exec python3 -c \"print('Hello, World!')\"".to_string());
        info.output = "Hello, World!\n".to_string();
        let out = tool_card_text(&info, Locale::ZhHans);
        assert!(
            out.starts_with("$ python3 -c \"print('Hello, World!')\""),
            "unexpected detail head: {out:?}"
        );
        assert!(out.contains("Hello, World!"));
        assert!(!out.contains("交互终端"));
        assert!(!out.contains("terminal_session exec"));
    }

    #[test]
    fn create_file_detail_shows_write_diff_and_strips_json_line_from_raw() {
        let mut info = mk("✅ create_file 成功");
        info.name = "create_file".to_string();
        let header = json!({
            "kind": "crabmate_tool_output",
            "tool": "create_file",
            "version": 1,
            "preview": "workspace_write_diff",
            "files": [{
                "path": "a.rs",
                "unified_diff": "--- a/a.rs\n+++ b/a.rs\n+fn x() {}\n",
                "truncated": false
            }],
            "preview_truncated": false
        });
        let line1 = serde_json::to_string(&header).unwrap();
        info.structured_preview = Some(header);
        info.output = format!("{line1}\n路径：a.rs\n已创建");
        let out = tool_card_text(&info, Locale::ZhHans);
        assert!(out.contains("变更预览"));
        assert!(out.contains("```diff"));
        assert!(!out.contains("crabmate_tool_output"));
        assert!(out.contains("路径：a.rs"));
    }

    #[test]
    fn run_command_success_detail_shell_title_skips_dup_cmd_line() {
        let mut info = mk("git diff --cached --stat");
        info.output =
            "命令：git diff --cached --stat\n退出码：0\n标准输出：\n  foo.rs | 1 +\n".to_string();
        let out = tool_card_text(&info, Locale::ZhHans);
        assert!(
            out.starts_with("$ git diff --cached --stat"),
            "unexpected head: {out:?}"
        );
        assert!(
            !out.contains("命令执行"),
            "should not use generic title: {out}"
        );
        assert!(
            !out.contains("命令：git diff"),
            "should drop duplicate 命令 line: {out}"
        );
        assert!(out.contains("退出码：0"), "{out}");
        assert!(out.contains("foo.rs"), "{out}");
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
    fn failed_non_whitelist_tool_appends_full_output_block() {
        let mut info = mk("");
        info.name = "http_fetch".to_string();
        info.summary = Some("❌ http_fetch 失败: timeout".to_string());
        info.ok = Some(false);
        info.output = "connection reset\nGET https://example.com failed".to_string();
        info.error_code = Some("timeout".to_string());
        let out = tool_card_text(&info, Locale::ZhHans);
        assert!(
            out.contains("完整输出"),
            "expected heading in detail text: {out}"
        );
        assert!(out.contains("connection reset"));
        assert!(out.contains("GET https://example.com failed"));
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

    #[test]
    fn compact_strips_stream_placeholder_running_suffix_zh() {
        let mut info = mk("");
        info.name = "git_log".to_string();
        info.summary = Some("git log · 工具执行中…".to_string());
        let out = tool_card_compact_text(&info, Locale::ZhHans);
        assert!(!out.contains("工具执行中"), "不应保留流式占位后缀: {out:?}");
    }

    #[test]
    fn compact_strips_stream_placeholder_running_suffix_en() {
        let mut info = mk("");
        info.name = "git_log".to_string();
        info.summary = Some("git log · Running tools…".to_string());
        let out = tool_card_compact_text(&info, Locale::En);
        assert!(
            !out.contains("Running tools"),
            "should not keep stream placeholder suffix: {out:?}"
        );
    }
}
