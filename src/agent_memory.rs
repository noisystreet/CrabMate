//! 工作区内的**长期备忘**文件：在首轮消息中注入为 `user` 条，便于模型记住用户/项目约定（可编辑 `.crabmate/agent_memory.md` 等）。

use std::path::Path;

use crate::path_workspace::absolutize_relative_under_root;
use crate::types::{Message, messages_chat_seed};

/// 读取 `rel_path`（相对工作区根）的 UTF-8 文本；不存在或越界返回 `None`。超长按 `max_chars` 截断（字符边界安全）。
pub fn load_memory_snippet(
    workspace_root: &Path,
    rel_path: &str,
    max_chars: usize,
) -> Option<String> {
    let rel = rel_path.trim();
    if rel.is_empty() || max_chars == 0 {
        return None;
    }
    let file_path = absolutize_relative_under_root(workspace_root, rel).ok()?;
    let raw = std::fs::read_to_string(&file_path).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    let mut body: String = t.chars().take(max_chars).collect();
    if t.chars().count() > max_chars {
        body.push_str("\n\n[... 备忘文件过长，已按 agent_memory_file_max_chars 截断 ...]");
    }
    Some(format!(
        "[用户/项目长期备忘（工作区内 {rel}，可直接编辑该文件更新）]\n{body}"
    ))
}

/// 与 [`messages_chat_seed`] 相同，但若 `memory_snippet` 为 `Some` 则在 `system` 与最后一条用户消息之间插入一条 `user`（备忘）。
pub fn messages_chat_seed_with_memory(
    system_prompt: &str,
    user_text: &str,
    memory_snippet: Option<&str>,
) -> Vec<Message> {
    let Some(mem) = memory_snippet.filter(|s| !s.trim().is_empty()) else {
        return messages_chat_seed(system_prompt, user_text);
    };
    vec![
        Message::system_only(system_prompt.to_string()),
        Message::user_only(mem.to_string()),
        Message::user_only(user_text.to_string()),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn memory_inserted_between_system_and_user() {
        let v = messages_chat_seed_with_memory("sys", "hello", Some("prefer rust"));
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].role, "system");
        assert_eq!(v[1].role, "user");
        assert!(v[1].content.as_deref().unwrap().contains("prefer rust"));
        assert_eq!(v[2].content.as_deref(), Some("hello"));
    }

    #[test]
    fn load_from_temp_workspace() {
        let dir =
            std::env::temp_dir().join(format!("crabmate_agent_memory_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let sub = dir.join("memo.md");
        let mut f = std::fs::File::create(&sub).unwrap();
        writeln!(f, "  use edition 2024  ").unwrap();
        let got = load_memory_snippet(&dir, "memo.md", 1000).unwrap();
        assert!(got.contains("edition 2024"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
