//! 工作区 `.crabmate/tui_session.json`：加载/保存与导出（实现委托 `runtime::chat_export`）。
//! CLI REPL 使用 `initial_workspace_messages`；保存/导出快捷键随后续终端 UI 再接回。
#![allow(dead_code)]

use crate::config::AgentConfig;
use crate::runtime::chat_export::{self, ChatSessionFile, session_to_json_pretty};
use crate::types::{Message, normalize_messages_for_openai_compatible_request};
use std::path::{Path, PathBuf};

pub fn session_file_path(workspace: &Path) -> PathBuf {
    workspace.join(".crabmate").join("tui_session.json")
}

/// 超过上限时保留首条 `system` 与尾部最近若干条，减轻启动与 TUI 渲染负担。
///
/// 若保留的尾部以**两条连续** `assistant` 开头，且丢弃的前缀里仍有 `user`，会把**该前缀中最后一条 user** 插回尾部，
/// 避免变成 `[system, assistant, assistant, …]`（供应商报 `Invalid consecutive assistant message at message index 2`）。
fn truncate_loaded_messages(mut msgs: Vec<Message>, max_total: usize) -> Vec<Message> {
    let max_total = max_total.max(2);
    if msgs.len() <= max_total {
        return normalize_messages_for_openai_compatible_request(msgs);
    }
    let system = msgs.remove(0);
    let after = msgs;
    let tail_keep = max_total.saturating_sub(1);
    let skip = after.len().saturating_sub(tail_keep);
    let mut tail: Vec<Message> = after.iter().skip(skip).cloned().collect();

    let tail_opens_with_assistant_run = tail.len() >= 2
        && tail[0].role.trim().eq_ignore_ascii_case("assistant")
        && tail[1].role.trim().eq_ignore_ascii_case("assistant");
    if tail_opens_with_assistant_run
        && let Some(ui) = after[..skip]
            .iter()
            .rposition(|m| m.role.trim().eq_ignore_ascii_case("user"))
    {
        tail.insert(0, after[ui].clone());
        while tail.len() > tail_keep {
            if tail.len() <= 1 {
                break;
            }
            tail.remove(1);
        }
    }

    let mut out = vec![system];
    out.extend(tail);
    normalize_messages_for_openai_compatible_request(out)
}

/// 若存在会话文件则加载；首条 `system` 会替换为当前配置的 `system_prompt`。
/// `max_messages` 为加载后的消息条数上限（含 `system`）；超出则丢弃最旧的用户/助手/工具消息。
pub fn load_workspace_session(
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

/// TUI / CLI REPL 启动时：按配置决定是否从磁盘恢复会话，否则仅一条 `system`（与当前 `system_prompt` 对齐）。
pub fn initial_workspace_messages(
    cfg: &AgentConfig,
    workspace: &Path,
    load_from_disk: bool,
) -> Vec<Message> {
    if !load_from_disk {
        return vec![Message::system_only(cfg.system_prompt.clone())];
    }
    load_workspace_session(workspace, &cfg.system_prompt, cfg.tui_session_max_messages)
        .unwrap_or_else(|| vec![Message::system_only(cfg.system_prompt.clone())])
}

pub fn save_workspace_session(workspace: &Path, messages: &[Message]) -> std::io::Result<()> {
    let dir = workspace.join(".crabmate");
    std::fs::create_dir_all(&dir)?;
    let path = session_file_path(workspace);
    let body = ChatSessionFile::new(messages.to_vec());
    let json = session_to_json_pretty(&body).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)
}

pub fn export_json(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_json_export(workspace, messages)
}

pub fn export_markdown(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    chat_export::write_markdown_export(workspace, messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.to_string()),
            reasoning_content: None,
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

    #[test]
    fn truncate_inserts_user_when_tail_would_be_two_assistants() {
        let v = vec![
            msg("system", "s"),
            msg("user", "old_u"),
            msg("assistant", "a1"),
            msg("assistant", "a2"),
        ];
        let t = truncate_loaded_messages(v, 3);
        assert_eq!(t.len(), 3);
        assert_eq!(t[0].role, "system");
        assert_eq!(t[1].role, "user");
        assert_eq!(t[1].content.as_deref(), Some("old_u"));
        assert_eq!(t[2].role, "assistant");
    }
}
