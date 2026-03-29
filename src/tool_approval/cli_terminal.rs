//! CLI 下工具审批：**TTY** 用 **dialoguer** 菜单（输出在 **stderr**，减少与 stdout 模型流交错）；**管道 / 无头** 回退为读一行并按 **`y` / `a` / `n`** 解析（与历史行为一致，便于脚本）。

use std::io::{self, BufRead, IsTerminal};

use dialoguer::Select;
use dialoguer::console::Term;
use dialoguer::theme::{ColorfulTheme, SimpleTheme};

use crate::types::CommandApprovalDecision;

/// 与 `tool_registry` 单测及管道回退共用。
pub(super) fn parse_cli_command_approval_line(line: &str) -> CommandApprovalDecision {
    let t = line.trim().to_ascii_lowercase();
    match t.as_str() {
        "" | "n" | "no" | "deny" | "d" | "q" => CommandApprovalDecision::Deny,
        "a" | "always" | "all" => CommandApprovalDecision::AllowAlways,
        _ => CommandApprovalDecision::AllowOnce,
    }
}

fn read_cli_command_approval_line_blocking() -> CommandApprovalDecision {
    let mut line = String::new();
    let _ = io::stdin().lock().read_line(&mut line);
    parse_cli_command_approval_line(&line)
}

/// **stdin** 与 **stderr** 均为 TTY 时可用箭头键菜单；否则走 [`read_cli_command_approval_line_blocking`]。
fn cli_tool_approval_use_dialoguer() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

fn select_decision_from_index(idx: usize) -> CommandApprovalDecision {
    match idx {
        0 => CommandApprovalDecision::Deny,
        1 => CommandApprovalDecision::AllowOnce,
        2 => CommandApprovalDecision::AllowAlways,
        _ => CommandApprovalDecision::Deny,
    }
}

fn prompt_tool_approval_dialoguer(title: &str, detail: &str) -> CommandApprovalDecision {
    const ITEMS: &[&str] = &[
        "拒绝（n / 回车）",
        "本次允许（y）",
        "永久允许该键，本会话（a）",
    ];
    let prompt = format!("[{title}]\n{detail}");
    let result = if std::env::var_os("NO_COLOR").is_some() {
        Select::with_theme(&SimpleTheme)
            .with_prompt(prompt)
            .items(ITEMS)
            .default(0)
            .interact_on(&Term::stderr())
    } else {
        Select::with_theme(&ColorfulTheme::default())
            .with_prompt(prompt)
            .items(ITEMS)
            .default(0)
            .interact_on(&Term::stderr())
    };
    match result {
        Ok(i) => select_decision_from_index(i),
        Err(_) => CommandApprovalDecision::Deny,
    }
}

fn print_fallback_instruction(title: &str, detail: &str) {
    eprintln!(
        "\n[{title}]\n{detail}\n  （非交互终端）输入 y 执行一次 | a 永久允许（本会话）| 其它或回车拒绝\n",
        title = title,
        detail = detail,
    );
}

/// `spawn_blocking` 内调用同步版本；异步上下文请用 [`prompt_tool_approval_cli`].
fn prompt_tool_approval_cli_blocking(title: &str, detail: &str) -> CommandApprovalDecision {
    if cli_tool_approval_use_dialoguer() {
        prompt_tool_approval_dialoguer(title, detail)
    } else {
        print_fallback_instruction(title, detail);
        read_cli_command_approval_line_blocking()
    }
}

/// 终端工具审批：**TTY** 菜单；否则打印说明并读一行。
pub(super) async fn prompt_tool_approval_cli(title: &str, detail: &str) -> CommandApprovalDecision {
    let title = title.to_string();
    let detail = detail.to_string();
    tokio::task::spawn_blocking(move || prompt_tool_approval_cli_blocking(&title, &detail))
        .await
        .unwrap_or(CommandApprovalDecision::Deny)
}

#[cfg(test)]
mod tests {
    use super::{parse_cli_command_approval_line, select_decision_from_index};
    use crate::types::CommandApprovalDecision;

    #[test]
    fn cli_approval_line_parsing() {
        assert_eq!(
            parse_cli_command_approval_line(""),
            CommandApprovalDecision::Deny
        );
        assert_eq!(
            parse_cli_command_approval_line("n"),
            CommandApprovalDecision::Deny
        );
        assert_eq!(
            parse_cli_command_approval_line("y"),
            CommandApprovalDecision::AllowOnce
        );
        assert_eq!(
            parse_cli_command_approval_line("YES "),
            CommandApprovalDecision::AllowOnce
        );
        assert_eq!(
            parse_cli_command_approval_line("a"),
            CommandApprovalDecision::AllowAlways
        );
        assert_eq!(
            parse_cli_command_approval_line("always"),
            CommandApprovalDecision::AllowAlways
        );
    }

    #[test]
    fn select_index_maps_decision() {
        assert_eq!(select_decision_from_index(0), CommandApprovalDecision::Deny);
        assert_eq!(
            select_decision_from_index(1),
            CommandApprovalDecision::AllowOnce
        );
        assert_eq!(
            select_decision_from_index(2),
            CommandApprovalDecision::AllowAlways
        );
        assert_eq!(
            select_decision_from_index(99),
            CommandApprovalDecision::Deny
        );
    }
}
