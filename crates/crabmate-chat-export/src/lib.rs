//! 会话导出契约：JSON 信封常量 / [`ChatSessionFile`]，以及 Markdown 分段标题与组装（无 I/O）。
//!
//! - **CLI / TUI / `save-session`**：经 `crabmate_runtime::chat_export` 落盘完整 OpenAI 形 [`Message`]。
//! - **Web / Tauri**：`frontend` 的 `session_export` 复用本 crate 的 schema 常量与 MD 标题；
//!   消息体可为展示投影（瘦 ExportMessage），与落盘完整会话并存。

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
    CHAT_EXPORT_SCHEMA_ID, CHAT_EXPORT_SCHEMA_VERSION, CHAT_SESSION_FILE_VERSION, ChatSessionFile,
    session_to_json_pretty,
};
