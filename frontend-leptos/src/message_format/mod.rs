//! 消息与工具摘要的展示用字符串处理（含 `agent_reply_plan` 围栏与流式缓冲语义）。
//!
//! - [`plain`]：与角色无关的行级整理
//! - [`tool_card`]：工具结果卡片单行摘要
//! - [`staged_timeline`]：分阶段时间线 `system` 前缀
//! - [`display`]：助手/用户/系统正文管道（思维链、`message_text_for_display_ex` 等）

mod display;
mod plain;
mod staged_timeline;
mod tool_card;

pub(crate) use display::{
    assistant_text_for_display, assistant_thinking_body_and_answer_raw,
    filter_assistant_thinking_markers_for_display, message_text_for_display_ex,
    stored_message_is_staged_planner_round,
};
pub(crate) use staged_timeline::{
    STAGED_TIMELINE_SYSTEM_PREFIX, is_staged_timeline_stored_message,
    staged_timeline_system_message_body,
};
pub(crate) use tool_card::tool_card_text;
