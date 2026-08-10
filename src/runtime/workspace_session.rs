//! 工作区历史文件名 `.crabmate/tui_session.json` 路径，以及 save-session / tool-replay 导出。
//! 不再提供 REPL/TUI 会话加载、保存或 bootstrap。

use crate::runtime::chat_export;
use crate::types::Message;
use std::path::{Path, PathBuf};

pub fn session_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".crabmate").join("tui_session.json")
}

pub fn export_json_with_projection(
    workspace: &Path,
    messages: &[Message],
    projection: chat_export::JsonExportProjection,
) -> std::io::Result<PathBuf> {
    chat_export::write_json_export_with_projection(workspace, messages, projection)
}

pub fn export_markdown(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_markdown_export(workspace, messages)
}
