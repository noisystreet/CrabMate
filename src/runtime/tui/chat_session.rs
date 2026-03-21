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

/// 若存在会话文件则加载；首条 `system` 会替换为当前配置的 `system_prompt`。
pub(super) fn load_tui_session(workspace: &Path, system_prompt: &str) -> Option<Vec<Message>> {
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
    Some(msgs)
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
