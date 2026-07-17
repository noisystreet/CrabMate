//! `chat` 子命令与交互式 REPL：按职责拆分为 `repl_parse`（`/…` 解析）、`commands`（`save-session` / `tool-replay`）、`chat`（`run_chat_invocation`）、`repl_extras`（斜杠命令入口与导出）、`repl_slash_dispatch`（斜杠命令分派实现）、`repl`（主循环）。

mod chat;
mod commands;
#[cfg(feature = "repl")]
mod repl;
#[cfg(feature = "repl")]
mod repl_extras;
#[cfg(feature = "repl")]
pub(crate) mod repl_parse;
#[cfg(feature = "repl")]
mod repl_slash_dispatch;

pub use chat::{CliMainInvocationCommon, run_chat_invocation};
pub use commands::{
    run_plugin_init_command, run_plugin_list_command, run_plugin_validate_command,
    run_save_session_command, run_sse_replay_command, run_tool_replay_command,
};
#[cfg(feature = "repl")]
pub use repl::run_repl;
#[cfg(feature = "repl")]
pub(crate) use repl::{
    ReplAfterUserMessageEnqueuedCb, ReplDispatchChatRoundParams, ReplSlashFollowupCtx,
    repl_dispatch_chat_round, repl_prepare_messages_and_editor, repl_slash_handled_followup,
};
#[cfg(feature = "repl")]
pub(crate) use repl_extras::{
    ReplSlashHandled, ReplSlashSharedHandles, try_handle_repl_slash_command,
};
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

#[cfg(not(feature = "repl"))]
pub async fn run_repl(
    _common: CliMainInvocationCommon<'_>,
    _no_stream: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "本 crabmate 二进制未启用 `repl` Cargo feature，不支持交互式 REPL。请使用默认构建或 `cargo build --features repl`。",
    )
    .into())
}
