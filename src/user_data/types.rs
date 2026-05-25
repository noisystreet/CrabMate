//! `~/.local/share/crabmate` JSON 契约（与 `docs/design/user_data_dir.md` 对齐）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserDataMeta {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub migrated_from: Vec<String>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

fn default_schema_version() -> u32 {
    SCHEMA_VERSION
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserPrefs {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_workspace_root: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_panel_view: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side_width: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub editor_layout_mode: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeline_panel_expanded: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_rail_collapsed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ui_font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_chat_font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide_editor_font: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide_editor_font_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide_editor_line_numbers: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide_editor_word_wrap: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ide_editor_tab_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bg_decor: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_bar_visible: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cm_role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disable_readonly_tool_ttl_cache: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmEndpointOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_context_tokens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_thinking_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LlmOverridesFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub client_llm: LlmEndpointOverride,
    #[serde(default)]
    pub executor_llm: LlmEndpointOverride,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub saved_models: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSessionsFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default)]
    pub sessions: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_session_id: Option<String>,
}

impl Default for WebSessionsFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            sessions: Value::Array(vec![]),
            active_session_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceManifest {
    pub workspace_root: String,
    #[serde(default)]
    pub normalized: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceListEntry {
    pub hash: String,
    pub workspace_root: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecretSlotStatus {
    pub set: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suffix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecretsStatusResponse {
    pub client_llm: SecretSlotStatus,
    pub executor_llm: SecretSlotStatus,
    pub web_api_bearer: SecretSlotStatus,
}
