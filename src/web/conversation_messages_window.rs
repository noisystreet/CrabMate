//! `GET /conversation/messages` 分页窗口：对过滤后的客户端可见消息切片。

use crate::types::Message;

/// 默认每页条数（Web 首屏水合与尾部刷新；与 [`frontend::conversation_messages_page::CONVERSATION_MESSAGES_PAGE_LIMIT`] 对齐）。
#[allow(dead_code)]
pub const DEFAULT_CONVERSATION_MESSAGES_PAGE_LIMIT: u32 = 80;
/// 单页上限（防止过大响应）。
pub const MAX_CONVERSATION_MESSAGES_PAGE_LIMIT: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClientMessageWindowMeta {
    pub total_count: u32,
    pub window_start_index: u32,
    pub has_older: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClientMessageWindowSlice {
    pub meta: ClientMessageWindowMeta,
    pub messages: Vec<Message>,
}

/// 对 `all` 切片。`limit` 为 `None` 或 `0` 时返回全量（兼容旧客户端）。
#[must_use]
pub fn slice_messages_for_client_window(
    all: &[Message],
    limit: Option<u32>,
    before_index: Option<u32>,
) -> ClientMessageWindowSlice {
    let total = u32::try_from(all.len()).unwrap_or(u32::MAX);
    let full = |items: Vec<Message>| ClientMessageWindowSlice {
        meta: ClientMessageWindowMeta {
            total_count: total,
            window_start_index: 0,
            has_older: false,
        },
        messages: items,
    };
    let Some(lim_raw) = limit.filter(|&n| n > 0) else {
        return full(all.to_vec());
    };
    let lim = lim_raw.clamp(1, MAX_CONVERSATION_MESSAGES_PAGE_LIMIT) as usize;
    let end = before_index
        .map(|i| i.min(total) as usize)
        .unwrap_or(all.len());
    let end = end.min(all.len());
    let start = end.saturating_sub(lim);
    ClientMessageWindowSlice {
        meta: ClientMessageWindowMeta {
            total_count: total,
            window_start_index: u32::try_from(start).unwrap_or(u32::MAX),
            has_older: start > 0,
        },
        messages: all[start..end].to_vec(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, MessageContent};

    fn user_msg(text: &str) -> Message {
        Message {
            role: "user".to_string(),
            content: Some(MessageContent::from(text)),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn no_limit_returns_full() {
        let all = vec![user_msg("a"), user_msg("b"), user_msg("c")];
        let w = slice_messages_for_client_window(&all, None, None);
        assert_eq!(w.messages.len(), 3);
        assert!(!w.meta.has_older);
        assert_eq!(w.meta.window_start_index, 0);
    }

    #[test]
    fn tail_page_takes_last_n() {
        let all: Vec<_> = (0..10).map(|i| user_msg(&format!("m{i}"))).collect();
        let w = slice_messages_for_client_window(&all, Some(3), None);
        assert_eq!(w.messages.len(), 3);
        assert!(w.meta.has_older);
        assert_eq!(w.meta.window_start_index, 7);
        assert_eq!(w.meta.total_count, 10);
    }

    #[test]
    fn before_index_loads_older_page() {
        let all: Vec<_> = (0..10).map(|i| user_msg(&format!("m{i}"))).collect();
        let w = slice_messages_for_client_window(&all, Some(3), Some(7));
        assert_eq!(w.messages.len(), 3);
        assert!(w.meta.has_older);
        assert_eq!(w.meta.window_start_index, 4);
    }
}
