//! 会话列表标题：与 Web/Tauri [`title_from_user_prompt`] 同源算法（首条用户消息压平截断）。
//!
//! Web 侧自定义重命名仅存浏览器 `ChatSession`，不进 SQLite；TUI 只能从落盘消息推导。

use crate::types::{Message, message_content_plain_for_chat_display};

/// 与 frontend `DEFAULT_CHAT_SESSION_TITLE` 存储默认对应的中文展示（TUI 侧栏文案为中文）。
pub const DEFAULT_SESSION_TITLE_ZH: &str = "新会话";

/// 首条用户消息 → 侧栏标题（压平换行、折叠空白，最长 48 字，超出加省略号）。
///
/// 与 `frontend/src/session_ops.rs` 的 `title_from_user_prompt` 保持一致。
pub fn title_from_user_prompt(text: &str) -> String {
    let t = text.trim();
    if t.is_empty() {
        return DEFAULT_SESSION_TITLE_ZH.to_string();
    }
    let single_line: String = t
        .chars()
        .map(|c| if matches!(c, '\n' | '\r') { ' ' } else { c })
        .collect();
    let collapsed = single_line.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_CHARS: usize = 48;
    let n = collapsed.chars().count();
    if n <= MAX_CHARS {
        collapsed
    } else {
        format!(
            "{}…",
            collapsed
                .chars()
                .take(MAX_CHARS.saturating_sub(1))
                .collect::<String>()
        )
    }
}

/// 从会话消息取首条用户内容生成标题；无用户消息时返回默认「新会话」。
pub fn conversation_title_from_messages(messages: &[Message]) -> String {
    for m in messages {
        if m.role != "user" {
            continue;
        }
        let plain = message_content_plain_for_chat_display(&m.content);
        if plain.trim().is_empty() {
            continue;
        }
        return title_from_user_prompt(&plain);
    }
    DEFAULT_SESSION_TITLE_ZH.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Message;

    #[test]
    fn title_matches_frontend_flatten_and_truncate() {
        assert_eq!(title_from_user_prompt("  hello\nworld  "), "hello world");
        assert_eq!(title_from_user_prompt("  \n\t  "), DEFAULT_SESSION_TITLE_ZH);
        let long = "字".repeat(60);
        let out = title_from_user_prompt(&long);
        assert!(out.ends_with('…'), "{out}");
        assert_eq!(out.chars().count(), 48);
    }

    #[test]
    fn conversation_title_uses_first_user() {
        let msgs = vec![
            Message::system_only("sys".to_string()),
            Message::user_only("分析 README".to_string()),
            Message::assistant_only("ok".to_string()),
        ];
        assert_eq!(conversation_title_from_messages(&msgs), "分析 README");
        assert_eq!(
            conversation_title_from_messages(&[Message::system_only("s".to_string())]),
            DEFAULT_SESSION_TITLE_ZH
        );
    }
}
