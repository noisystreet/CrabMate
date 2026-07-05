//! 聊天主路径：主列视图、输入与流式、`wire_*` 接线（滚动、查找、时间线）。
//!
//! 对应 `docs/frontend/ARCHITECTURE.md` 中 **`app/chat_*`** 域；[`ChatSessionSignals`](crate::chat_session_state::ChatSessionSignals) 仍在 crate 根以便与会话模态等共用。

mod column;
mod column_keyboard;
mod composer;
mod composer_follow_up;
mod composer_input_stack;
mod composer_mirror;
mod composer_stream;
mod composer_wires;
mod find;
mod find_bar;
mod handles;
mod message_chunks;
mod message_group_views;
mod message_row;
mod message_row_actions;
mod message_row_user_layout;
mod messages_list;
mod scroll;
mod scroll_follow;
mod scroll_shell;
mod session_hydrate;
mod session_merge;
mod session_storage;
mod stream_follow_up_gates;
mod stream_user_abort;
mod tail_loading_memo;
pub(crate) mod wire_chat_domain;
pub(crate) mod wire_chat_session_lifecycle;

pub use handles::{ChatColumnShell, ComposerStreamShell};

pub(crate) use handles::ChatComposerWires;

pub(crate) use column::chat_column_view;
pub(crate) use find_bar::ChatFindBar;
