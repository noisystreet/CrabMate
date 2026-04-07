//! `chat` 子命令与交互式 REPL：按职责拆分为 `repl_parse`（`/…` 解析）、`commands`（`save-session` / `tool-replay`）、`chat`（`run_chat_invocation`）、`repl_extras`（斜杠命令处理与导出）、`repl`（主循环）。

mod chat;
mod commands;
mod repl;
mod repl_extras;
pub(crate) mod repl_parse;

pub use chat::run_chat_invocation;
pub use commands::{run_save_session_command, run_tool_replay_command};
pub use repl::run_repl;

/// REPL `/export`、`/save-session` 与 `save-session` 子命令共用的导出格式。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ReplExportKind {
    Json,
    Markdown,
    Both,
}

use std::path::PathBuf;

pub(crate) fn cli_effective_work_dir(workspace_cli: &Option<String>, default: &str) -> PathBuf {
    PathBuf::from(
        workspace_cli
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(default),
    )
}
