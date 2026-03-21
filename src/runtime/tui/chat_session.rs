//! TUI 会话：工作区 `.crabmate/tui_session.json` 与导出（实现委托 `chat_export`）。

use crate::chat_export::{self, ChatSessionFile, session_to_json_pretty};
use crate::types::Message;
use std::path::{Path, PathBuf};

pub(super) fn session_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".crabmate").join("tui_session.json")
}

/// 超过上限时保留首条 `system` 与尾部最近若干条，减轻启动与 TUI 渲染负担。
fn truncate_loaded_messages(mut msgs: Vec<Message>, max_total: usize) -> Vec<Message> {
    let max_total = max_total.max(2);
    if msgs.len() <= max_total {
        return msgs;
    }
    let system = msgs.remove(0);
    let tail_keep = max_total.saturating_sub(1);
    let skip = msgs.len().saturating_sub(tail_keep);
    let mut out = vec![system];
    out.extend(msgs.into_iter().skip(skip));
    out
}

/// 若存在会话文件则加载；首条 `system` 会替换为当前配置的 `system_prompt`。
/// `max_messages` 为加载后的消息条数上限（含 `system`）；超出则丢弃最旧的用户/助手/工具消息。
pub(super) fn load_tui_session(
    workspace: &Path,
    system_prompt: &str,
    max_messages: usize,
) -> Option<Vec<Message>> {
    let path = session_file_path(workspace);
    let data = std::fs::read_to_string(&path).ok()?;
    let parsed: ChatSessionFile = serde_json::from_str(&data).ok()?;
    if parsed.messages.is_empty() {
        return None;
    }
    let mut msgs = parsed.messages;
    if msgs.first().is_some_and(|m| m.role == "system") {
        msgs[0].content = Some(system_prompt.to_string());
    } else {
        msgs.insert(0, Message::system_only(system_prompt.to_string()));
    }
    Some(truncate_loaded_messages(msgs, max_messages))
}

pub(super) fn save_tui_session(workspace: &Path, messages: &[Message]) -> std::io::Result<()> {
    let dir = workspace.join(".crabmate");
    std::fs::create_dir_all(&dir)?;
    let path = session_file_path(workspace);
    let body = ChatSessionFile::new(messages.to_vec());
    let json = session_to_json_pretty(&body).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)
}

pub(super) fn export_json(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_json_export(workspace, messages)
}

pub(super) fn export_markdown(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_markdown_export(workspace, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn truncate_keeps_system_and_tail() {
        let v = vec![
            msg("system", "s"),
            msg("user", "a"),
            msg("assistant", "b"),
            msg("user", "c"),
            msg("assistant", "d"),
        ];
        let t = truncate_loaded_messages(v, 3);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].role, "system");
        assert_eq!(t[1].content.as_deref(), Some("c"));
        assert_eq!(t[2].content.as_deref(), Some("d"));
    }

    #[test]
    fn truncate_noop_when_short() {
        let v = vec![msg("system", "s"), msg("user", "u")];
        let t = truncate_loaded_messages(v, 10);
        assert_eq!(t.len(), 2);
    }
}
