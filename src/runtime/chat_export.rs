//! 会话导出：与 `.crabmate/tui_session.json` 同形的 JSON，以及 Markdown 文本生成。
//! 供 `runtime/workspace_session` 使用；Web 前端 `frontend-leptos/src/session_export.rs` 应对齐 `CHAT_SESSION_FILE_VERSION` 与字段含义。
#![allow(dead_code)]

use crate::types::Message;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// 与磁盘 `tui_session.json`、导出 `chat_export_*.json` 的版本号一致；演进时同步修改前端常量。
pub const CHAT_SESSION_FILE_VERSION: u32 = 1;

/// OpenAI 兼容消息列表外包一层版本号，供持久化与导出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionFile {
    pub version: u32,
    pub messages: Vec<Message>,
}

impl ChatSessionFile {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            version: CHAT_SESSION_FILE_VERSION,
            messages,
        }
    }

    pub fn from_slice(messages: &[Message]) -> Self {
        Self {
            version: CHAT_SESSION_FILE_VERSION,
            messages: messages.to_vec(),
        }
    }
}

pub fn session_to_json_pretty(file: &ChatSessionFile) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(file)
}

/// 与 TUI F9 / Web 导出一致：跳过 `system` 角色；`tool` 与 `assistant`/`user` 分段输出。
pub fn messages_to_markdown(messages: &[Message]) -> String {
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
        let body = if m.role == "assistant" {
            crate::runtime::message_display::assistant_raw_markdown_body_for_message(m)
        } else {
            crate::types::message_content_as_str(&m.content)
                .unwrap_or("")
                .to_string()
        };
        md.push_str(&body);
        md.push_str("\n\n");
    }
    md
}

/// `<workspace>/.crabmate/exports`
pub fn workspace_exports_dir(workspace: &Path) -> PathBuf {
    workspace.join(".crabmate").join("exports")
}

fn export_filename(prefix: &str, ext: &str) -> String {
    format!(
        "{}_{}.{}",
        prefix,
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        ext
    )
}

/// 写入 `exports/chat_export_YYYYMMDD_HHMMSS.json`。
pub fn write_json_export(workspace: &Path, messages: &[Message]) -> io::Result<PathBuf> {
    let dir = workspace_exports_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(export_filename("chat_export", "json"));
    let body = ChatSessionFile::from_slice(messages);
    let json = session_to_json_pretty(&body).map_err(io::Error::other)?;
    std::fs::write(&path, json)?;
    Ok(path)
}

/// 写入 `exports/chat_export_YYYYMMDD_HHMMSS.md`。
pub fn write_markdown_export(workspace: &Path, messages: &[Message]) -> io::Result<PathBuf> {
    let dir = workspace_exports_dir(workspace);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(export_filename("chat_export", "md"));
    std::fs::write(&path, messages_to_markdown(messages))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn markdown_skips_system_and_labels_roles() {
        let md = messages_to_markdown(&[
            msg("system", "sys"),
            msg("user", "hi"),
            msg("assistant", "hey"),
            msg("tool", "out"),
        ]);
        assert!(!md.contains("sys"));
        assert!(md.contains("## 用户"));
        assert!(md.contains("hi"));
        assert!(md.contains("## 助手"));
        assert!(md.contains("## 工具"));
        assert!(md.contains("out"));
    }

    #[test]
    fn session_file_roundtrip() {
        let file = ChatSessionFile::new(vec![msg("user", "x")]);
        let s = session_to_json_pretty(&file).unwrap();
        let back: ChatSessionFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.version, CHAT_SESSION_FILE_VERSION);
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].role, "user");
    }
}
