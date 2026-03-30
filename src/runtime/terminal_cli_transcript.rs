//! CLI 无 SSE 通道（`out: None`）时，将分阶段规划与工具结果写到 stdout，与 TUI 聊天区信息对齐。
//! 前缀/次要行颜色与 [`crate::runtime::cli_repl_ui::CliReplStyle`] 的 **`/help`** 主题 RGB 同源（见 **`CLI_REPL_HELP_*_FG`**），并同样尊重 **`NO_COLOR`**、非 TTY 不着色。

use log::debug;

use crate::redact;
use crate::runtime::cli_repl_ui::{
    CLI_REPL_HELP_CMD_FG, CLI_REPL_HELP_DESC_FG, CLI_REPL_HELP_TITLE_FG, cli_repl_stdout_use_color,
};
use crate::tool_result::{ParsedLegacyOutput, parse_legacy_output};

use crossterm::{
    queue,
    style::{Attribute, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

use super::latex_unicode::latex_math_to_unicode;
use super::message_display::{tool_content_for_display_full, user_message_for_chat_display};

/// 若 `detail` 以「与工具名同义的动词短语 + `:`」开头（工具名 `snake_case` 视作空格分词），则只去掉该动词短语，**保留冒号**与后续内容（如 `create file: x` → `: x`），避免 `### 工具 · create_file create file: x` 的重复。
///
/// 仅用于 CLI 标题；SSE/Web 仍使用完整 `summarize_tool_call` 文案。含 URL 等中间冒号的摘要仅在**首段**形如 `verb:` 且与工具名匹配时才会剥离。
pub(crate) fn strip_redundant_tool_summary_prefix(tool_name: &str, detail: &str) -> String {
    let s = detail.trim();
    if s.is_empty() || tool_name.is_empty() {
        return s.to_string();
    }
    let verbal = tool_name.replace('_', " ");
    let verbal_key = verbal.to_lowercase();
    let Some(colon_byte) = s.find(':') else {
        return s.to_string();
    };
    let (head, after) = s.split_at(colon_byte);
    let tail = after.strip_prefix(':').unwrap_or(after).trim();
    if head.trim().is_empty() {
        return s.to_string();
    }
    if head.trim().to_lowercase() == verbal_key {
        if tail.is_empty() {
            s.to_string()
        } else {
            format!(": {tail}")
        }
    } else {
        s.to_string()
    }
}

/// CLI 工具标题中，工具名与详情之间统一使用 **` : `**；若 `detail` 已以 `:` 开头（如去重后的 `: path`）则不再插入第二个冒号。
fn terminal_tool_title_suffix_after_name(detail: &str) -> Option<String> {
    let t = detail.trim();
    if t.is_empty() {
        return None;
    }
    Some(if t.starts_with(':') {
        format!(" {t}")
    } else {
        format!(" : {t}")
    })
}

/// 生成 `### 工具 · name …` 行中名称后的摘要片段（与 SSE `tool_call.summary` 同源；无摘要时用单行截断的 `args`）。
pub(crate) fn tool_result_header_detail(args: &str, summary: Option<&str>) -> Option<String> {
    if let Some(s) = summary {
        let t = s.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    let t = args.trim();
    if t.is_empty() {
        return None;
    }
    let single_line: String = t
        .chars()
        .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })
        .collect();
    let single_line = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX: usize = 160;
    if single_line.chars().count() > MAX {
        Some(format!(
            "{}…",
            single_line
                .chars()
                .take(MAX.saturating_sub(1))
                .collect::<String>()
        ))
    } else {
        Some(single_line)
    }
}

/// 与 TUI 聊天区展示一致：正文经 **`user_message_for_chat_display`**（含分步注入 user 长句压缩、LaTeX），再按行打印到 stdout。
///
/// `clear_before` 时先空一行，并对**首条非空行**加粗 + [`CLI_REPL_HELP_TITLE_FG`]（与 **`/help`** 节标题同级）；其余非空行用 [`CLI_REPL_HELP_DESC_FG`]。
pub(crate) fn print_staged_plan_notice(clear_before: bool, text: &str) -> io::Result<()> {
    let display = user_message_for_chat_display(text);
    debug!(
        target: "crabmate::print",
        "CLI 打印分阶段规划通知 clear_before={} text_len={} text_preview={}",
        clear_before,
        display.len(),
        redact::preview_chars(&display, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let mut w = io::stdout();
    let color = cli_repl_stdout_use_color();
    if clear_before {
        writeln!(w)?;
    }
    let mut highlight_first_nonempty = clear_before;
    for line in display.lines() {
        let t = line.trim_end();
        if t.is_empty() {
            continue;
        }
        if highlight_first_nonempty {
            highlight_first_nonempty = false;
            if color {
                queue!(
                    w,
                    SetAttribute(Attribute::Bold),
                    SetForegroundColor(CLI_REPL_HELP_TITLE_FG)
                )?;
            }
            writeln!(w, "{t}")?;
            if color {
                queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
            }
        } else if color {
            queue!(w, SetForegroundColor(CLI_REPL_HELP_DESC_FG))?;
            writeln!(w, "{t}")?;
            queue!(w, ResetColor)?;
        } else {
            writeln!(w, "{t}")?;
        }
    }
    w.flush()
}

const PLAYBOOK_HINT_SNIPPET_MAX: usize = 12_000;

/// 工具失败时于 CLI stdout 提示可一键诊断（`playbook_run_commands`）；**不**自动执行。
pub(crate) fn print_cli_playbook_healing_hint(
    tool_name: &str,
    raw_result: &str,
    parsed: &ParsedLegacyOutput,
) -> io::Result<()> {
    if matches!(
        tool_name,
        "playbook_run_commands" | "error_output_playbook" | "diagnostic_summary"
    ) || tool_name.starts_with("mcp__")
    {
        return Ok(());
    }
    let body = if !parsed.stderr.trim().is_empty() {
        parsed.stderr.as_str()
    } else if !parsed.stdout.trim().is_empty() {
        parsed.stdout.as_str()
    } else {
        raw_result
    };
    let body = body.trim();
    if body.is_empty() {
        return Ok(());
    }
    let take = body.len().min(PLAYBOOK_HINT_SNIPPET_MAX);
    let snippet = &body[..take];
    let json_snippet = serde_json::to_string(snippet).unwrap_or_else(|_| "\"\"".to_string());

    let mut w = io::stdout();
    let color = cli_repl_stdout_use_color();
    if color {
        queue!(
            w,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(CLI_REPL_HELP_TITLE_FG)
        )?;
    }
    writeln!(w, "\n── 自愈提示 · 诊断命令包 ──")?;
    if color {
        queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
        queue!(w, SetForegroundColor(CLI_REPL_HELP_DESC_FG))?;
    }
    writeln!(
        w,
        "可将下方整行交给模型调用工具 **playbook_run_commands**（参数 JSON 内 `error_text` 已转义）；或自行拆分 `run_command`。\n\
         请先**脱敏**（勿含 API Key、token、完整 Authorization 等）。"
    )?;
    writeln!(
        w,
        "{{\"error_text\":{json_snippet},\"ecosystem\":\"auto\"}}"
    )?;
    if color {
        queue!(w, ResetColor)?;
    }
    w.flush()
}

/// 工具执行结束后打印名称与正文（正文用完整格式化，与聊天区「可省略实际输出」策略独立），过长按 `max_chars` 截断（近似字符数）。
///
/// 标题行为 `### 工具 · {name}`；有详情时统一为 **`### 工具 · {name} : …`**（摘要已以 `:` 开头时不再重复冒号），例：`run_command` + `ls -la` → `### 工具 · run_command : ls -la`，`create_file` + 去重后 `: a.cpp` → `### 工具 · create_file : a.cpp`。
///
/// `omit_body` 为 true 时只打印标题与一行说明，**不**打印 `raw_result` 正文（保留供其它调用方；当前 `echo_tool_result_transcript` 对 **`read_file` / `read_dir` / `list_tree`** 均传入摘要正文并传 **`omit_body = false`**）。
pub(crate) fn print_tool_result_terminal(
    name: &str,
    args: &str,
    summary: Option<&str>,
    raw_result: &str,
    max_chars: usize,
    omit_body: bool,
) -> io::Result<()> {
    debug!(
        target: "crabmate::print",
        "CLI 打印工具结果 tool={} omit_body={} raw_len={} raw_preview={}",
        name,
        omit_body,
        raw_result.len(),
        redact::preview_chars(raw_result, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    let mut w = io::stdout();
    let color = cli_repl_stdout_use_color();
    let title_rest = tool_result_header_detail(args, summary).map(|d| {
        let compact = strip_redundant_tool_summary_prefix(name, &d);
        if compact.is_empty() { d } else { compact }
    });
    if color {
        queue!(
            w,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(CLI_REPL_HELP_CMD_FG)
        )?;
    }
    if let Some(rest) = title_rest.as_deref() {
        if let Some(suffix) = terminal_tool_title_suffix_after_name(rest) {
            writeln!(w, "\n### 工具 · {name}{suffix}")?;
        } else {
            writeln!(w, "\n### 工具 · {name}")?;
        }
    } else {
        writeln!(w, "\n### 工具 · {name}")?;
    }
    if color {
        queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
        queue!(w, SetForegroundColor(CLI_REPL_HELP_DESC_FG))?;
    }
    if omit_body {
        writeln!(w, "（已省略输出正文；完整内容在对话上下文）")?;
        if color {
            queue!(w, ResetColor)?;
        }
        w.flush()?;
        return Ok(());
    }

    let mut body = tool_content_for_display_full(raw_result);
    body = latex_math_to_unicode(&body);
    let n = body.chars().count();
    if n > max_chars {
        let take: String = body.chars().take(max_chars.saturating_sub(1)).collect();
        body = format!("{take}…\n[输出已截断，原约 {n} 字；完整内容已写入对话历史]");
    }
    writeln!(w, "{body}")?;
    if color {
        queue!(w, ResetColor)?;
    }
    debug!(
        target: "crabmate::print",
        "CLI 工具结果展示正文 body_len={} body_preview={}",
        body.len(),
        redact::preview_chars(&body, redact::MESSAGE_LOG_PREVIEW_CHARS)
    );
    w.flush()
}

/// CLI 下 `read_file` 终端展示：保留编码/行范围等元数据块，正文仅展示前若干行（与 `assemble_read_output` 的 `行号|内容` 格式对齐），避免刷屏；完整串仍写入对话历史。
#[must_use]
pub(crate) fn read_file_result_terminal_summary(raw: &str) -> String {
    let raw = raw.trim_end();
    if raw.is_empty() {
        return String::new();
    }
    // 短错误、参数问题：原样展示
    if raw.starts_with("错误：")
        || raw.starts_with("参数 JSON 无效")
        || raw.starts_with("缺少 path")
        || raw.starts_with("读取元数据失败")
        || raw.starts_with("打开文件失败")
        || raw.starts_with("文件为空:")
    {
        return raw.to_string();
    }
    const PREVIEW_LINES: usize = 16;
    const MAX_LINE_CHARS: usize = 256;
    let lines: Vec<&str> = raw.lines().collect();
    let mut content_start: Option<usize> = None;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        let Some((prefix, _rest)) = t.split_once('|') else {
            continue;
        };
        if prefix.trim().chars().all(|c| c.is_ascii_digit()) && !prefix.trim().is_empty() {
            content_start = Some(i);
            break;
        }
    }
    let Some(cs) = content_start else {
        return truncate_tool_output_with_note(raw, TERMINAL_TOOL_SUMMARY_FALLBACK_CHARS);
    };
    let header = lines[..cs].join("\n");
    let body_lines = &lines[cs..];
    let n_preview = body_lines.len().min(PREVIEW_LINES);
    let mut preview = String::new();
    for line in body_lines.iter().take(n_preview) {
        let line_len = line.chars().count();
        let lim: String = line.chars().take(MAX_LINE_CHARS).collect();
        preview.push_str(&lim);
        if line_len > MAX_LINE_CHARS {
            preview.push_str(" …(行内截断)");
        }
        preview.push('\n');
    }
    let more = body_lines.len().saturating_sub(n_preview);
    let more_note = if more > 0 {
        format!("尚有后续 {more} 行未在终端显示。")
    } else {
        "本段正文已在上方尽数展示。".to_string()
    };
    format!(
        "{header}\n\n---\n终端摘要：以下为正文前 {n_preview} 行（本段共 {} 行）。{more_note}\n\n{preview}（完整输出已写入本轮对话上下文。）",
        body_lines.len(),
    )
}

const TERMINAL_TOOL_SUMMARY_FALLBACK_CHARS: usize = 1600;

fn truncate_tool_output_with_note(raw: &str, max_chars: usize) -> String {
    let n = raw.chars().count();
    if n <= max_chars {
        return raw.to_string();
    }
    let mut s: String = raw.chars().take(max_chars.saturating_sub(80)).collect();
    s.push_str("\n…\n（输出过长已截断；完整内容在对话历史）");
    s
}

/// CLI 下 `read_dir`：保留「目录:」首行与「总计遍历」尾行，中间条目仅前若干行。
#[must_use]
pub(crate) fn read_dir_result_terminal_summary(raw: &str) -> String {
    let raw = raw.trim_end();
    if raw.is_empty() {
        return String::new();
    }
    if raw.starts_with("错误：")
        || raw.starts_with("参数 JSON 无效")
        || raw.starts_with("读取目录失败")
    {
        return raw.to_string();
    }
    const PREVIEW_LINES: usize = 24;
    const MAX_LINE_CHARS: usize = 200;
    let lines: Vec<&str> = raw.lines().collect();
    let Some(first) = lines.first().copied() else {
        return raw.to_string();
    };
    if !first.starts_with("目录:") {
        return truncate_tool_output_with_note(raw, TERMINAL_TOOL_SUMMARY_FALLBACK_CHARS);
    }
    let body_lines: Vec<&str> = lines
        .iter()
        .skip(1)
        .copied()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("dir: ") || t.starts_with("file: ")
        })
        .collect();
    let footer = lines.iter().rev().find(|l| l.contains("总计遍历")).copied();
    let n_preview = body_lines.len().min(PREVIEW_LINES);
    let mut preview = String::new();
    for line in body_lines.iter().take(n_preview) {
        let line_len = line.chars().count();
        let lim: String = line.chars().take(MAX_LINE_CHARS).collect();
        preview.push_str(&lim);
        if line_len > MAX_LINE_CHARS {
            preview.push_str(" …(行内截断)");
        }
        preview.push('\n');
    }
    let more = body_lines.len().saturating_sub(n_preview);
    let more_note = if more > 0 {
        format!("尚有后续 {more} 条条目未在终端显示。")
    } else {
        "本段条目已在上方尽数展示。".to_string()
    };
    let mut out = format!(
        "{first}\n\n---\n终端摘要：以下为前 {n_preview} 条（共 {} 条展示用条目）。{more_note}\n\n{preview}",
        body_lines.len(),
    );
    if let Some(f) = footer {
        out.push_str(&format!("\n---\n{f}\n（完整输出已写入本轮对话上下文。）"));
    } else {
        out.push_str("\n（完整输出已写入本轮对话上下文。）");
    }
    out
}

/// CLI 下 `list_tree`：保留起始参数块，树行仅前若干行，并保留末尾「共 N 条」统计（按 `\n---\n` 分段解析）。
#[must_use]
pub(crate) fn list_tree_result_terminal_summary(raw: &str) -> String {
    let raw = raw.trim_end();
    if raw.is_empty() {
        return String::new();
    }
    if raw.starts_with("错误：") || raw.starts_with("参数 JSON 无效") {
        return raw.to_string();
    }
    const PREVIEW_LINES: usize = 24;
    const MAX_LINE_CHARS: usize = 200;
    let parts: Vec<&str> = raw.split("\n---\n").collect();
    if parts.len() < 2 {
        return truncate_tool_output_with_note(raw, TERMINAL_TOOL_SUMMARY_FALLBACK_CHARS);
    }
    let meta = parts[0].trim_end();
    let mid = parts[1];
    let footer = parts.get(2).map(|s| s.trim()).filter(|s| !s.is_empty());
    let mid_lines: Vec<&str> = mid.lines().collect();
    let n_preview = mid_lines.len().min(PREVIEW_LINES);
    let mut preview = String::new();
    for line in mid_lines.iter().take(n_preview) {
        let line_len = line.chars().count();
        let lim: String = line.chars().take(MAX_LINE_CHARS).collect();
        preview.push_str(&lim);
        if line_len > MAX_LINE_CHARS {
            preview.push_str(" …(行内截断)");
        }
        preview.push('\n');
    }
    let more = mid_lines.len().saturating_sub(n_preview);
    let more_note = if more > 0 {
        format!("尚有后续 {more} 行未在终端显示。")
    } else {
        "树行已在上方尽数展示。".to_string()
    };
    let mut out = format!(
        "{meta}\n\n---\n终端摘要：以下为树输出前 {n_preview} 行（本段共 {} 行）。{more_note}\n\n{preview}",
        mid_lines.len(),
    );
    if let Some(f) = footer {
        out.push_str(&format!("\n---\n{f}\n（完整输出已写入本轮对话上下文。）"));
    } else {
        out.push_str("\n（完整输出已写入本轮对话上下文。）");
    }
    out
}

/// `agent_turn::execute_tools` 在 `echo_terminal_transcript` 为真时的 CLI 回显入口：打印工具标题/正文，并在**未**挂 SSE（`sse_attached == false`）且结果为失败时附加 [`print_cli_playbook_healing_hint`]。
pub(crate) fn echo_tool_result_transcript(
    echo: bool,
    sse_attached: bool,
    name: &str,
    args: &str,
    tool_summary: Option<&str>,
    result: &str,
    terminal_tool_display_max_chars: usize,
) {
    if !echo {
        return;
    }
    use std::borrow::Cow;
    let body_for_print: Cow<'_, str> = match name {
        "read_file" => Cow::Owned(read_file_result_terminal_summary(result)),
        "read_dir" => Cow::Owned(read_dir_result_terminal_summary(result)),
        "list_tree" => Cow::Owned(list_tree_result_terminal_summary(result)),
        _ => Cow::Borrowed(result),
    };
    let _ = print_tool_result_terminal(
        name,
        args,
        tool_summary,
        body_for_print.as_ref(),
        terminal_tool_display_max_chars,
        false,
    );
    if !sse_attached {
        let parsed_preview = parse_legacy_output(name, result);
        if !parsed_preview.ok {
            let _ = print_cli_playbook_healing_hint(name, result, &parsed_preview);
        }
    }
}

#[cfg(test)]
mod tool_header_detail_tests {
    use super::{
        strip_redundant_tool_summary_prefix, terminal_tool_title_suffix_after_name,
        tool_result_header_detail,
    };

    #[test]
    fn title_suffix_inserts_colon_before_detail() {
        assert_eq!(
            terminal_tool_title_suffix_after_name("ls -la").as_deref(),
            Some(" : ls -la")
        );
        assert_eq!(
            terminal_tool_title_suffix_after_name(": hello.cpp").as_deref(),
            Some(" : hello.cpp")
        );
        assert_eq!(
            terminal_tool_title_suffix_after_name("   ").as_deref(),
            None
        );
    }

    #[test]
    fn strip_prefix_drops_verb_when_matches_tool_name() {
        assert_eq!(
            strip_redundant_tool_summary_prefix("create_file", "create file: hello.cpp"),
            ": hello.cpp"
        );
        assert_eq!(
            strip_redundant_tool_summary_prefix("create_file", "Create File: hi.rs"),
            ": hi.rs"
        );
    }

    #[test]
    fn strip_prefix_keeps_unrelated_or_no_colon() {
        assert_eq!(
            strip_redundant_tool_summary_prefix("create_file", "delete file: x"),
            "delete file: x"
        );
        assert_eq!(
            strip_redundant_tool_summary_prefix("run_command", "ls -la"),
            "ls -la"
        );
    }

    #[test]
    fn strip_prefix_read_dir_matches_verbal_form() {
        assert_eq!(
            strip_redundant_tool_summary_prefix("read_dir", "read dir: ."),
            ": ."
        );
        assert_eq!(
            strip_redundant_tool_summary_prefix("read_dir", "read dir: src"),
            ": src"
        );
    }

    #[test]
    fn strip_prefix_empty_tail_keeps_full() {
        assert_eq!(
            strip_redundant_tool_summary_prefix("create_file", "create file:"),
            "create file:"
        );
    }

    #[test]
    fn uses_nonempty_summary() {
        assert_eq!(
            tool_result_header_detail(r#"{"command":"ls"}"#, Some("ls")).as_deref(),
            Some("ls")
        );
    }

    #[test]
    fn empty_summary_falls_back_to_args_one_line() {
        let d = tool_result_header_detail(r#"{"command":"pwd"}"#, Some("   ")).expect("detail");
        assert!(d.contains("command"));
    }

    #[test]
    fn long_args_truncated() {
        let inner = "x".repeat(200);
        let json = format!(r#"{{"p":"{inner}"}}"#);
        let d = tool_result_header_detail(&json, None).expect("detail");
        assert!(d.ends_with('…'));
        assert!(d.chars().count() <= 161);
    }
}

#[cfg(test)]
mod read_file_terminal_summary_tests {
    use super::read_file_result_terminal_summary;

    #[test]
    fn error_passthrough() {
        let s = "错误：路径不是文件或不存在，无法读取";
        assert_eq!(read_file_result_terminal_summary(s), s);
    }

    #[test]
    fn metadata_plus_preview_shows_all_when_short() {
        let raw = "文本编码: utf-8\n\
文件: src/lib.rs\n\
总行数: 100\n\
本段行范围: 1-5（单次 max_lines=500）\n\
已读到文件末尾（本段范围内无更多行）。\n\
\n\
1|alpha\n\
2|beta\n\
3|gamma\n";
        let out = read_file_result_terminal_summary(raw);
        assert!(out.contains("终端摘要"));
        assert!(out.contains("1|alpha"));
        assert!(out.contains("3|gamma"));
        assert!(out.contains("本段共 3 行"));
        assert!(
            out.contains("本段正文已在上方尽数展示"),
            "no '尚有后续' when all lines fit in preview"
        );
    }

    #[test]
    fn metadata_plus_preview_truncates_when_many_body_lines() {
        let mut raw = "文件: x\n总行数: 20\n本段行范围: 1-20\n\n".to_string();
        for i in 1..=20 {
            raw.push_str(&format!("{i}|line{i}\n"));
        }
        let out = read_file_result_terminal_summary(&raw);
        assert!(out.contains("本段共 20 行"));
        assert!(out.contains("尚有后续 4 行"));
        assert!(out.contains("1|line1"));
        assert!(out.contains("16|line16"));
        assert!(!out.contains("20|line20"));
    }

    #[test]
    fn no_numbered_lines_uses_fallback_truncation() {
        let body = "x".repeat(2000);
        let out = read_file_result_terminal_summary(&body);
        assert!(out.contains("…"));
        assert!(out.contains("对话历史"));
    }
}

#[cfg(test)]
mod read_dir_list_tree_terminal_summary_tests {
    use super::{list_tree_result_terminal_summary, read_dir_result_terminal_summary};

    #[test]
    fn read_dir_error_passthrough() {
        let s = "错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径";
        assert_eq!(read_dir_result_terminal_summary(s), s);
    }

    #[test]
    fn read_dir_summary_with_footer() {
        let raw = "目录: src\n\
file: a.rs\n\
dir: b\n\
总计遍历: 5，展示: 2";
        let out = read_dir_result_terminal_summary(raw);
        assert!(out.contains("目录: src"));
        assert!(out.contains("终端摘要"));
        assert!(out.contains("file: a.rs"));
        assert!(out.contains("总计遍历"));
        assert!(out.contains("对话上下文"));
    }

    #[test]
    fn read_dir_truncates_many_entries() {
        let mut raw = "目录: .\n".to_string();
        for i in 0..30 {
            raw.push_str(&format!("file: f{i}.txt\n"));
        }
        raw.push_str("总计遍历: 30，展示: 30");
        let out = read_dir_result_terminal_summary(&raw);
        assert!(out.contains("尚有后续 6 条"));
        assert!(out.contains("file: f0.txt"));
        assert!(!out.contains("file: f29.txt"));
        assert!(out.contains("总计遍历"));
    }

    #[test]
    fn list_tree_three_part_summary() {
        let raw = "起始目录（相对工作区）: .\nmax_depth=4 max_entries=400 include_hidden=false\n---\ndir: .\nfile: Cargo.toml\ndir: src/\n---\n共 3 条（含起点 .）";
        let out = list_tree_result_terminal_summary(raw);
        assert!(out.contains("起始目录"));
        assert!(out.contains("终端摘要"));
        assert!(out.contains("file: Cargo.toml"));
        assert!(out.contains("共 3 条"));
    }

    #[test]
    fn list_tree_many_lines_preview() {
        let mut mid = String::from("dir: .\n");
        for i in 0..30 {
            mid.push_str(&format!("file: p{i}\n"));
        }
        let raw = format!(
            "起始目录（相对工作区）: .\nmax_depth=2 max_entries=500 include_hidden=false\n---\n{mid}---\n共 31 条（含起点 .）"
        );
        let out = list_tree_result_terminal_summary(&raw);
        assert!(out.contains("尚有后续 7 行"));
        assert!(out.contains("file: p0"));
        assert!(!out.contains("file: p29"));
    }
}
