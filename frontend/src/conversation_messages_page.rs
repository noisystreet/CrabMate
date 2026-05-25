//! `GET /conversation/messages` 分页参数（与后端 [`DEFAULT_CONVERSATION_MESSAGES_PAGE_LIMIT`] 对齐）。

/// 与 `src/web/conversation_messages_window.rs` 默认页大小一致。
pub const CONVERSATION_MESSAGES_PAGE_LIMIT: u32 = 80;

#[derive(Clone, Copy, Debug, Default)]
pub struct ConversationMessagesFetchParams {
    pub limit: Option<u32>,
    pub before_index: Option<u32>,
}

impl ConversationMessagesFetchParams {
    #[must_use]
    pub const fn tail_page() -> Self {
        Self {
            limit: Some(CONVERSATION_MESSAGES_PAGE_LIMIT),
            before_index: None,
        }
    }

    #[must_use]
    pub const fn older_before(window_start_index: u32) -> Self {
        Self {
            limit: Some(CONVERSATION_MESSAGES_PAGE_LIMIT),
            before_index: Some(window_start_index),
        }
    }
}
