//! 运维 CLI：`save-session` / `tool-replay` / `sse-replay` / `plugin *` 等（同进程 `chat`/`repl`/`tui` 已于 D2.2 硬删）。

mod commands;

pub use commands::{
    run_plugin_init_command, run_plugin_list_command, run_plugin_validate_command,
    run_save_session_command, run_sse_replay_command, run_tool_replay_command,
};

/// `save-session` 子命令共用的导出格式。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SessionExportKind {
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
