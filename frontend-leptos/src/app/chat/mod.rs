//! 聊天主路径：主列视图、输入与流式、`wire_*` 接线（滚动、查找、时间线）。
//!
//! 对应 `docs/frontend-leptos/ARCHITECTURE.md` 中 **`app/chat_*`** 域；[`ChatSessionSignals`](crate::chat_session_state::ChatSessionSignals) 仍在 crate 根以便与会话模态等共用。

mod column;
mod composer;
mod composer_stream;
mod find;
mod find_bar;
mod handles;
mod message_chunks;
mod message_group_views;
mod message_row;
mod message_row_actions;
mod scroll;
mod staged_plan_todo;
mod timeline;
pub(crate) mod wire_chat_domain;

pub use handles::{ChatColumnShell, ComposerStreamShell};

pub(crate) use column::chat_column_view;
pub(crate) use find_bar::ChatFindBar;
pub(crate) use timeline::load_timeline_panel_expanded_default;
