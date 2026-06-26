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

/// `mcp_servers.json` 单条 stdio MCP 服务器（用户数据目录，非 TOML）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub id: String,
    pub name: String,
    pub slug: String,
    #[serde(default)]
    pub command: String,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Web `GET /user-data/mcp-servers`：不返回 `command`（仅 `has_command`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntryPublic {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    pub has_command: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Web `GET /user-data/mcp-servers` 响应体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServersFilePublic {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_mcp_global_enabled")]
    pub global_enabled: bool,
    #[serde(default = "default_mcp_tool_timeout_secs")]
    pub tool_timeout_secs: u64,
    #[serde(default)]
    pub servers: Vec<McpServerEntryPublic>,
}

impl From<&McpServersFile> for McpServersFilePublic {
    fn from(file: &McpServersFile) -> Self {
        Self {
            schema_version: file.schema_version,
            global_enabled: file.global_enabled,
            tool_timeout_secs: file.tool_timeout_secs,
            servers: file
                .servers
                .iter()
                .map(|s| McpServerEntryPublic {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    slug: s.slug.clone(),
                    enabled: s.enabled,
                    has_command: !s.command.trim().is_empty(),
                    created_at_ms: s.created_at_ms,
                    updated_at_ms: s.updated_at_ms,
                })
                .collect(),
        }
    }
}

/// `POST /user-data/mcp-servers/import` 响应。
#[derive(Debug, Clone, Serialize)]
pub struct McpServersImportResponse {
    pub file: McpServersFilePublic,
    pub imported_count: usize,
    pub warnings: Vec<String>,
    pub skipped_remote: Vec<String>,
}

/// `~/.local/share/crabmate/mcp_servers.json`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServersFile {
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,
    #[serde(default = "default_mcp_global_enabled")]
    pub global_enabled: bool,
    #[serde(default = "default_mcp_tool_timeout_secs")]
    pub tool_timeout_secs: u64,
    #[serde(default)]
    pub servers: Vec<McpServerEntry>,
}

fn default_mcp_global_enabled() -> bool {
    true
}

fn default_mcp_tool_timeout_secs() -> u64 {
    60
}

impl Default for McpServersFile {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            global_enabled: default_mcp_global_enabled(),
            tool_timeout_secs: default_mcp_tool_timeout_secs(),
            servers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRemoteToolSummary {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerStatusEntry {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    pub connected: bool,
    pub openai_tool_names: Vec<String>,
    pub remote_tools: Vec<McpRemoteToolSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServersStatusResponse {
    pub global_enabled: bool,
    pub tool_timeout_secs: u64,
    pub servers: Vec<McpServerStatusEntry>,
}
