//! 用户消息行内联 flex（与 `layout-chat.css` 中 `.msg-row-user` 配对）及工具卡附带数据。

use leptos::prelude::*;

use crate::session_ops::preceding_plain_user_message_id;
use crate::storage::ChatSession;

pub(super) fn tool_bubble_detail_and_jump_uid(
    is_tool_bubble: bool,
    reasoning_text: String,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
    msg_idx: usize,
) -> (Option<String>, Option<String>) {
    if !is_tool_bubble {
        return (None, None);
    }
    let jump_uid = sessions.with(|list| {
        let aid = active_id.get();
        list.iter()
            .find(|s| s.id == aid)
            .and_then(|sess| preceding_plain_user_message_id(&sess.messages, msg_idx))
    });
    (Some(reasoning_text), jump_uid)
}

/// 用户消息右对齐：**内联**保证不被其它样式盖掉。
const USER_ROW_OUTER_STYLE: &str = concat!(
    "display:flex !important;flex-flow:row nowrap !important;",
    "justify-content:flex-end !important;align-items:flex-start !important;",
    "width:100% !important;max-width:100% !important;box-sizing:border-box !important;",
    "align-self:stretch !important;",
);
/// 与用户气泡同宽、防止 flex 把栈压扁导致正文一字一行。
const USER_ROW_STACK_STYLE: &str = concat!(
    "display:flex !important;flex-direction:column !important;",
    "align-items:flex-end !important;width:fit-content !important;max-width:100% !important;",
    "flex-shrink:0 !important;min-width:auto !important;box-sizing:border-box !important;",
);
const USER_ROW_BUBBLE_STYLE: &str = concat!(
    "box-sizing:border-box !important;min-width:auto !important;flex-shrink:0 !important;",
    "width:fit-content !important;max-width:100% !important;",
);

pub(super) fn chat_row_wrap_and_user_styles(
    role: &str,
    is_tool_bubble: bool,
) -> (&'static str, &'static str, &'static str, &'static str) {
    if role != "user" {
        return ("msg-with-select", "", "", "");
    }
    let bubble = if is_tool_bubble {
        ""
    } else {
        USER_ROW_BUBBLE_STYLE
    };
    (
        "msg-with-select msg-row-user",
        USER_ROW_OUTER_STYLE,
        USER_ROW_STACK_STYLE,
        bubble,
    )
}
