//! 会话导出契约：JSON 信封常量 / [`ChatSessionFile`] / [`DisplayChatSessionFile`]，以及 Markdown 分段（无 I/O）。
//!
//! - **CLI / TUI / `save-session`**：[`ChatSessionFile`]（`projection=raw`）完整 OpenAI 形 [`Message`]。
//! - **Web / Tauri**：[`DisplayChatSessionFile`]（`projection=display`）展示投影；**不可**直接作 tool-replay 输入。

mod markdown;
mod schema;

pub use markdown::{
    ExportMdLocale, append_markdown_role_section, export_md_heading_assistant,
    export_md_heading_other, export_md_heading_timeline, export_md_heading_tool,
    export_md_heading_user, export_md_title_full, export_md_title_selection,
    markdown_from_role_bodies, messages_to_markdown_with_body,
    reorder_messages_for_conversation_flow,
};
pub use schema::{
    CHAT_EXPORT_PROJECTION_DISPLAY, CHAT_EXPORT_PROJECTION_RAW, CHAT_EXPORT_SCHEMA_ID,
    CHAT_EXPORT_SCHEMA_VERSION, CHAT_SESSION_FILE_VERSION, ChatSessionFile, DisplayChatSessionFile,
    DisplayExportMessage, display_session_to_json_pretty, ensure_raw_projection,
    projection_is_display, projection_is_raw, session_to_json_pretty,
};
