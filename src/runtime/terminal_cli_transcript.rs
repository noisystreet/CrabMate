//! CLI 无 SSE 通道（`out: None`）时，将分阶段规划与工具结果写到 stdout，与 TUI 聊天区信息对齐。
//! 前缀/次要行颜色与 [`crate::runtime::cli_repl_ui::CliReplStyle`] 的 **`/help`** 主题 RGB 同源（见 **`CLI_REPL_HELP_*_FG`**），并同样尊重 **`NO_COLOR`**、非 TTY 不着色。

use log::debug;

use crate::redact;
use crate::runtime::cli_repl_ui::{
    CLI_REPL_HELP_CMD_FG, CLI_REPL_HELP_DESC_FG, CLI_REPL_HELP_TITLE_FG, cli_repl_stdout_use_color,
};

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

/// 工具执行结束后打印名称与正文（正文用完整格式化，与聊天区「可省略实际输出」策略独立），过长按 `max_chars` 截断（近似字符数）。
///
/// 标题行为 `### 工具 · {name}`；有详情时统一为 **`### 工具 · {name} : …`**（摘要已以 `:` 开头时不再重复冒号），例：`run_command` + `ls -la` → `### 工具 · run_command : ls -la`，`create_file` + 去重后 `: a.cpp` → `### 工具 · create_file : a.cpp`。
///
/// `omit_body` 为 true 时只打印标题与一行说明，**不**打印 `raw_result` 正文（用于 `read_file` / `list_tree` 等易刷屏工具；完整结果仍由调用方写入对话历史）。
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
