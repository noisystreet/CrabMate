//! JSON 信封：与 `.crabmate/tui_session.json` / `chat_export_*.json` 同形。
//!
//! ## 投影（`projection`）
//!
//! | 值 | 含义 | `messages` 形状 | 典型生产者 |
//! |----|------|-----------------|------------|
//! | [`CHAT_EXPORT_PROJECTION_RAW`] | 完整 OpenAI 兼容 [`Message`] | 含 `tool_calls` / `reasoning_*` 等 | CLI/TUI `save-session`、会话落盘 |
//! | [`CHAT_EXPORT_PROJECTION_DISPLAY`] | 展示投影 | 瘦 [`DisplayExportMessage`] | Web/Tauri 下载 |
//!
//! **`schema` / `schema_version` / `projection` / `version` / `messages` 均为必填**；缺键即反序列化失败（**不**兼容旧 JSON）。
//! `tool-replay` 等工具链**只接受** [`CHAT_EXPORT_PROJECTION_RAW`]。

use crate::cm_types::Message;
use serde::{Deserialize, Serialize};

/// 与磁盘 `tui_session.json`、导出 `chat_export_*.json` 的消息数组约定版本；破坏性变更时递增。
pub const CHAT_SESSION_FILE_VERSION: u32 = 1;

/// 顶层 JSON 信封的稳定标识（URI 形），与 [`CHAT_EXPORT_SCHEMA_VERSION`] 一起用于工具链与排障。
pub const CHAT_EXPORT_SCHEMA_ID: &str = "crabmate.chat_session";

/// 信封 SemVer；破坏性改 envelope（含取消缺字段兼容）时 bump major。
pub const CHAT_EXPORT_SCHEMA_VERSION: &str = "2.0.0";

/// 完整 OpenAI 形消息列表（CLI/TUI / tool-replay 输入）。
pub const CHAT_EXPORT_PROJECTION_RAW: &str = "raw";

/// Web/Tauri 展示过滤后的瘦消息列表（**不可**直接喂给 tool-replay）。
pub const CHAT_EXPORT_PROJECTION_DISPLAY: &str = "display";

/// `projection` 是否为完整 raw 会话（大小写不敏感）。
#[must_use]
pub fn projection_is_raw(projection: &str) -> bool {
    projection
        .trim()
        .eq_ignore_ascii_case(CHAT_EXPORT_PROJECTION_RAW)
}

/// `projection` 是否为展示投影。
#[must_use]
pub fn projection_is_display(projection: &str) -> bool {
    projection
        .trim()
        .eq_ignore_ascii_case(CHAT_EXPORT_PROJECTION_DISPLAY)
}

/// 若不是 raw 投影则返回说明错误（供 tool-replay 等拒绝 display 导出）。
pub fn ensure_raw_projection(projection: &str) -> Result<(), String> {
    if projection_is_raw(projection) {
        return Ok(());
    }
    Err(format!(
        "该文件 projection={projection:?} 不是完整会话（需要 {CHAT_EXPORT_PROJECTION_RAW:?}）；Web 展示导出请用 CLI/TUI save-session 或会话落盘 JSON"
    ))
}

/// OpenAI 兼容消息列表外包一层版本号，供持久化与 **raw** 导出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionFile {
    /// 固定为 [`CHAT_EXPORT_SCHEMA_ID`]。
    pub schema: String,
    /// 与 [`CHAT_EXPORT_SCHEMA_ID`] 配对的 SemVer 字符串。
    pub schema_version: String,
    /// [`CHAT_EXPORT_PROJECTION_RAW`] 或 [`CHAT_EXPORT_PROJECTION_DISPLAY`]。
    pub projection: String,
    pub version: u32,
    pub messages: Vec<Message>,
}

impl ChatSessionFile {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            schema: CHAT_EXPORT_SCHEMA_ID.to_string(),
            schema_version: CHAT_EXPORT_SCHEMA_VERSION.to_string(),
            projection: CHAT_EXPORT_PROJECTION_RAW.to_string(),
            version: CHAT_SESSION_FILE_VERSION,
            messages,
        }
    }

    pub fn from_slice(messages: &[Message]) -> Self {
        Self::new(messages.to_vec())
    }
}

/// Web/Tauri 导出用的瘦消息（展示正文；无 `tool_calls` 等 API 字段）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DisplayExportMessage {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// 与 [`ChatSessionFile`] 同信封键，但 `projection=display` 且消息为展示投影。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayChatSessionFile {
    pub schema: String,
    pub schema_version: String,
    pub projection: String,
    pub version: u32,
    pub messages: Vec<DisplayExportMessage>,
}

impl DisplayChatSessionFile {
    pub fn new(messages: Vec<DisplayExportMessage>) -> Self {
        Self {
            schema: CHAT_EXPORT_SCHEMA_ID.to_string(),
            schema_version: CHAT_EXPORT_SCHEMA_VERSION.to_string(),
            projection: CHAT_EXPORT_PROJECTION_DISPLAY.to_string(),
            version: CHAT_SESSION_FILE_VERSION,
            messages,
        }
    }
}

pub fn session_to_json_pretty(file: &ChatSessionFile) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(file)
}

pub fn display_session_to_json_pretty(
    file: &DisplayChatSessionFile,
) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::Message;

    fn msg(role: &str, content: &str) -> Message {
        Message {
            role: role.to_string(),
            content: Some(content.into()),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn session_file_roundtrip_includes_raw_projection() {
        let file = ChatSessionFile::new(vec![msg("user", "x")]);
        let s = session_to_json_pretty(&file).unwrap();
        assert!(s.contains(CHAT_EXPORT_SCHEMA_ID));
        assert!(s.contains(CHAT_EXPORT_SCHEMA_VERSION));
        assert!(s.contains(CHAT_EXPORT_PROJECTION_RAW));
        let back: ChatSessionFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.schema, CHAT_EXPORT_SCHEMA_ID);
        assert_eq!(back.schema_version, CHAT_EXPORT_SCHEMA_VERSION);
        assert_eq!(back.projection, CHAT_EXPORT_PROJECTION_RAW);
        assert_eq!(back.version, CHAT_SESSION_FILE_VERSION);
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].role, "user");
        assert!(ensure_raw_projection(&back.projection).is_ok());
    }

    #[test]
    fn session_file_rejects_missing_envelope_fields() {
        let json = r#"{"version":1,"messages":[]}"#;
        assert!(serde_json::from_str::<ChatSessionFile>(json).is_err());
        let no_projection = r#"{"schema":"crabmate.chat_session","schema_version":"2.0.0","version":1,"messages":[]}"#;
        assert!(serde_json::from_str::<ChatSessionFile>(no_projection).is_err());
    }

    #[test]
    fn display_file_marks_projection_and_rejects_as_raw() {
        let file = DisplayChatSessionFile::new(vec![DisplayExportMessage {
            role: "user".into(),
            content: Some("hi".into()),
            name: None,
        }]);
        let s = display_session_to_json_pretty(&file).unwrap();
        assert!(s.contains(CHAT_EXPORT_PROJECTION_DISPLAY));
        assert!(ensure_raw_projection(&file.projection).is_err());
        let back: DisplayChatSessionFile = serde_json::from_str(&s).unwrap();
        assert!(projection_is_display(&back.projection));
        assert_eq!(back.messages[0].content.as_deref(), Some("hi"));
    }

    #[test]
    fn empty_projection_is_not_raw() {
        assert!(!projection_is_raw(""));
        assert!(ensure_raw_projection("").is_err());
    }
}
