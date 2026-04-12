//! 聊天主路径：主列视图、输入与流式、`wire_*` 接线（滚动、查找、时间线）。
//!
//! 对应 `docs/frontend-leptos/ARCHITECTURE.md` 中 **`app/chat_*`** 域；[`ChatSessionSignals`](crate::chat_session_state::ChatSessionSignals) 仍在 crate 根以便与会话模态等共用。

mod column;
mod composer;
mod find;
mod find_bar;
mod message_chunks;
mod message_render;
mod scroll;
mod timeline;

pub(crate) use column::chat_column_view;
pub(crate) use composer::{
    wire_chat_composer_streams, wire_draft_sync_to_buffer_and_textarea,
    wire_session_switch_clears_chat_state,
};
pub(crate) use find::wire_chat_find_matches;
pub(crate) use find_bar::ChatFindBar;
pub(crate) use scroll::{wire_focus_message_after_nav, wire_messages_auto_scroll};
pub(crate) use timeline::load_timeline_panel_expanded_default;
