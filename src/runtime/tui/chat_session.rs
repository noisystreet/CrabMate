//! TUI 会话：工作区 `.crabmate/tui_session.json` 与导出。

use crate::types::Message;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
struct TuiSessionFile {
    version: u32,
    messages: Vec<Message>,
}

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
    let parsed: TuiSessionFile = serde_json::from_str(&data).ok()?;
    if parsed.messages.is_empty() {
        return None;
    }
    let mut msgs = parsed.messages;
    if msgs.first().is_some_and(|m| m.role == "system") {
        msgs[0].content = Some(system_prompt.to_string());
    } else {
        msgs.insert(
            0,
            Message {
                role: "system".to_string(),
                content: Some(system_prompt.to_string()),
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        );
    }
    Some(truncate_loaded_messages(msgs, max_messages))
}

pub(super) fn save_tui_session(workspace: &Path, messages: &[Message]) -> std::io::Result<()> {
    let dir = workspace.join(".crabmate");
    std::fs::create_dir_all(&dir)?;
    let path = session_file_path(workspace);
    let body = TuiSessionFile {
        version: 1,
        messages: messages.to_vec(),
    };
    let json = serde_json::to_string_pretty(&body).map_err(std::io::Error::other)?;
    std::fs::write(&path, json)
}

pub(super) fn export_json(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    let dir = workspace.join(".crabmate").join("exports");
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "chat_export_{}.json",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let path = dir.join(name);
    let body = TuiSessionFile {
        version: 1,
        messages: messages.to_vec(),
    };
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&body).map_err(std::io::Error::other)?,
    )?;
    Ok(path)
}

pub(super) fn export_markdown(workspace: &Path, messages: &[Message]) -> std::io::Result<PathBuf> {
    let dir = workspace.join(".crabmate").join("exports");
    std::fs::create_dir_all(&dir)?;
    let name = format!(
        "chat_export_{}.md",
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let path = dir.join(name);
    let mut md = String::from("# CrabMate 聊天记录\n\n");
    for m in messages {
        if m.role == "system" {
            continue;
        }
        let heading = match m.role.as_str() {
            "user" => "## 用户",
            "assistant" => "## 助手",
            "tool" => "## 工具",
            _ => "## 其它",
        };
        md.push_str(heading);
        md.push_str("\n\n");
        md.push_str(m.content.as_deref().unwrap_or(""));
        md.push_str("\n\n");
    }
    std::fs::write(&path, md)?;
    Ok(path)
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
