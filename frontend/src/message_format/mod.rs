//! 消息与工具摘要的展示用字符串处理（含 `agent_reply_plan` 围栏与流式缓冲语义）。
//!
//! - [`plain`]：与角色无关的行级整理
//! - [`tool_card`]：工具结果卡片单行摘要
//! - [`staged_timeline`]：分阶段时间线 `system` 前缀
//! - [`display`]：助手/用户/系统正文管道（内分 `plan_fence` / `thinking_strip` / `message_ex`）

mod display;
mod dsml_strip;
mod plain;
mod staged_timeline;
mod stored_message;
mod strip_ansi;
mod tool_card;
mod tool_envelope;

#[cfg(test)]
pub(crate) use display::assistant_text_for_display;
pub(crate) use display::{
    assistant_message_text_for_display_ex_with_body_strings, message_text_for_display_ex,
};
#[cfg(test)]
pub(crate) use display::{
    assistant_thinking_body_and_answer_raw, filter_assistant_thinking_markers_for_display,
};
pub(crate) use staged_timeline::{
    STAGED_TIMELINE_SYSTEM_PREFIX, staged_timeline_system_message_body,
};
pub(crate) use stored_message::tool_stored_text_from_result_info;
pub(crate) use strip_ansi::strip_ansi_codes;
pub(crate) use tool_envelope::{
    format_tool_role_content_for_stored_message, stored_tool_message_compact_text,
    stored_tool_message_detail_text, tool_result_info_from_stored_content,
};
