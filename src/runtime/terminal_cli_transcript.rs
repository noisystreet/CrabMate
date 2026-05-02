//! CLI 无 SSE 通道（`out: None`）时，将分阶段规划与工具结果写到 stdout，与 TUI 聊天区信息对齐。
//! 前缀/次要行颜色与 [`crate::runtime::cli_repl_ui::CliReplStyle`] 的 **`/help`** 主题 RGB 同源（见 **`CLI_REPL_HELP_*_FG`**），并同样尊重 **`NO_COLOR`**、非 TTY 不着色。
//!
//! 实现拆至 [`terminal_cli_transcript_impl`]，避免 `lizard` 将后续函数误并入首个函数（扭曲 `fn-nloc`）。

#[path = "terminal_cli_transcript_impl.rs"]
mod terminal_cli_transcript_impl;

use crabmate_sse_protocol::StreamEndReason;

use crate::runtime::cli_repl_ui::{CLI_REPL_HELP_TITLE_FG, cli_repl_stdout_use_color};
use crossterm::{
    queue,
    style::{Attribute, ResetColor, SetAttribute, SetForegroundColor},
};
use std::io::{self, Write};

// 供 `crate::runtime::terminal_cli_transcript::*` 调用方使用；本文件不直接引用。
#[allow(unused_imports)]
pub(crate) use terminal_cli_transcript_impl::{
    echo_tool_result_transcript, list_tree_result_terminal_summary,
    print_cli_playbook_healing_hint, print_staged_plan_notice, print_tool_result_terminal,
    read_dir_result_terminal_summary, read_file_result_terminal_summary,
    rust_file_outline_result_terminal_short, search_in_files_result_terminal_short,
    tool_result_header_detail,
};

/// CLI 回合收尾提示：统一展示终止原因（同源于 `StreamEndReason`），便于与 Web/TUI 对齐排障。
pub(crate) fn print_stream_end_reason_terminal(reason: StreamEndReason) -> io::Result<()> {
    let mut w = io::stdout();
    let color = cli_repl_stdout_use_color();
    if color {
        queue!(
            w,
            SetAttribute(Attribute::Bold),
            SetForegroundColor(CLI_REPL_HELP_TITLE_FG)
        )?;
    }
    writeln!(
        w,
        "\n── 回合结束：{} ({}) ──",
        reason.label_zh_hans(),
        reason.as_str()
    )?;
    if color {
        queue!(w, SetAttribute(Attribute::Reset), ResetColor)?;
    }
    w.flush()
}

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
    // 使用 `if let` 而非 `let ... else`，以便 `lizard`/`fn-nloc` 等静态分析正确闭合函数体。
    if let Some(colon_byte) = s.find(':') {
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
    } else {
        s.to_string()
    }
}
