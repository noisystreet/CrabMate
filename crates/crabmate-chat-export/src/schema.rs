//! JSON 信封：与 `.crabmate/tui_session.json` / `chat_export_*.json` 同形。

use crabmate_types::Message;
use serde::{Deserialize, Serialize};

/// 与磁盘 `tui_session.json`、导出 `chat_export_*.json` 的消息数组约定版本；破坏性变更时递增。
pub const CHAT_SESSION_FILE_VERSION: u32 = 1;

/// 顶层 JSON 信封的稳定标识（URI 形），与 [`CHAT_EXPORT_SCHEMA_VERSION`] 一起用于工具链与排障。
pub const CHAT_EXPORT_SCHEMA_ID: &str = "crabmate.chat_session";

/// 信封 SemVer；仅当 `schema` 不变而信封字段或语义兼容扩展时可 bump patch；破坏性改 envelope 时 bump minor/major。
pub const CHAT_EXPORT_SCHEMA_VERSION: &str = "1.0.0";

fn default_chat_export_schema() -> String {
    CHAT_EXPORT_SCHEMA_ID.to_string()
}

fn default_chat_export_schema_version() -> String {
    CHAT_EXPORT_SCHEMA_VERSION.to_string()
}

/// OpenAI 兼容消息列表外包一层版本号，供持久化与导出。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionFile {
    /// 固定为 [`CHAT_EXPORT_SCHEMA_ID`]；旧文件缺该键时反序列化默认填充，便于读旧 `tui_session.json`。
    #[serde(default = "default_chat_export_schema")]
    pub schema: String,
    /// 与 [`CHAT_EXPORT_SCHEMA_ID`] 配对的 SemVer 字符串。
    #[serde(default = "default_chat_export_schema_version")]
    pub schema_version: String,
    pub version: u32,
    pub messages: Vec<Message>,
}

impl ChatSessionFile {
    pub fn new(messages: Vec<Message>) -> Self {
        Self {
            schema: CHAT_EXPORT_SCHEMA_ID.to_string(),
            schema_version: CHAT_EXPORT_SCHEMA_VERSION.to_string(),
            version: CHAT_SESSION_FILE_VERSION,
            messages,
        }
    }

    pub fn from_slice(messages: &[Message]) -> Self {
        Self {
            schema: CHAT_EXPORT_SCHEMA_ID.to_string(),
            schema_version: CHAT_EXPORT_SCHEMA_VERSION.to_string(),
            version: CHAT_SESSION_FILE_VERSION,
            messages: messages.to_vec(),
        }
    }
}

pub fn session_to_json_pretty(file: &ChatSessionFile) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(file)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::Message;

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
    fn session_file_roundtrip() {
        let file = ChatSessionFile::new(vec![msg("user", "x")]);
        let s = session_to_json_pretty(&file).unwrap();
        assert!(s.contains(CHAT_EXPORT_SCHEMA_ID));
        assert!(s.contains(CHAT_EXPORT_SCHEMA_VERSION));
        let back: ChatSessionFile = serde_json::from_str(&s).unwrap();
        assert_eq!(back.schema, CHAT_EXPORT_SCHEMA_ID);
        assert_eq!(back.schema_version, CHAT_EXPORT_SCHEMA_VERSION);
        assert_eq!(back.version, CHAT_SESSION_FILE_VERSION);
        assert_eq!(back.messages.len(), 1);
        assert_eq!(back.messages[0].role, "user");
    }

    #[test]
    fn session_file_deserialize_legacy_without_schema() {
        let json = r#"{"version":1,"messages":[]}"#;
        let f: ChatSessionFile = serde_json::from_str(json).unwrap();
        assert_eq!(f.schema, CHAT_EXPORT_SCHEMA_ID);
        assert_eq!(f.schema_version, CHAT_EXPORT_SCHEMA_VERSION);
        assert_eq!(f.version, 1);
        assert!(f.messages.is_empty());
    }
}
