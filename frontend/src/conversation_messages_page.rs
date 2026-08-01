//! `GET /conversation/messages` 分页参数。

/// 前端默认页大小（与历史后端默认 80 对齐；上限见后端 `MAX_CONVERSATION_MESSAGES_PAGE_LIMIT`）。
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
