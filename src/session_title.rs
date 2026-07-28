//! 会话列表标题：与 Web/Tauri [`title_from_user_prompt`] 同源算法（首条**真实**用户消息压平截断）。
//!
//! 跳过长期记忆 / 首轮工作区画像等服务端注入 `user`（与 [`user_message_counts_for_branch_truncation`] 一致），
//! 避免侧栏标题变成「以下为与当前问题可能相关的长期记忆…」或长期停留在「新会话」。
//!
//! Web 侧自定义重命名仅存浏览器 `ChatSession`，不进 SQLite；TUI 只能从落盘消息推导。

use crate::types::{
    Message, message_content_plain_for_chat_display, user_message_counts_for_branch_truncation,
};

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

/// 从会话消息取首条**真实**用户内容生成标题；无则返回默认「新会话」。
pub fn conversation_title_from_messages(messages: &[Message]) -> String {
    for m in messages {
        if !user_message_counts_for_branch_truncation(m) {
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
    use crate::types::{
        CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME, CRABMATE_LONG_TERM_MEMORY_NAME, Message,
    };

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

    #[test]
    fn conversation_title_skips_long_term_memory_and_first_turn_inject() {
        let mut mem = Message::user_only(
            "以下为与当前问题可能相关的长期记忆（【经验 #id】为可复用提炼；[记忆 #id] 为回合摘要；可用 long_term_memory_list 核对；若无关请忽略）：\n\n- foo"
                .to_string(),
        );
        mem.name = Some(CRABMATE_LONG_TERM_MEMORY_NAME.to_string());
        let mut ctx = Message::user_only("## 项目画像\n\ncrate = crabmate".to_string());
        ctx.name = Some(CRABMATE_FIRST_TURN_WORKSPACE_CONTEXT_NAME.to_string());
        let msgs = vec![
            Message::system_only("sys".to_string()),
            mem,
            ctx,
            Message::user_only("修一下编译错误".to_string()),
        ];
        assert_eq!(conversation_title_from_messages(&msgs), "修一下编译错误");
    }

    #[test]
    fn conversation_title_skips_memory_by_content_prefix_without_name() {
        // 无 name 的历史落盘：靠 display_rules 内容识别
        let mem = Message::user_only(
            "以下为与当前问题可能相关的长期记忆（【经验 #id】为可复用提炼；[记忆 #id] 为回合摘要；可用 long_term_memory_list 核对；若无关请忽略）：\n\n- bar"
                .to_string(),
        );
        let msgs = vec![
            Message::system_only("sys".to_string()),
            mem,
            Message::user_only("实现纯色块".to_string()),
        ];
        assert_eq!(conversation_title_from_messages(&msgs), "实现纯色块");
    }
}
