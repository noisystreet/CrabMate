//! `~/.local/share/crabmate` JSON 契约（与 `docs/design/user_data_dir.md` 对齐）。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crabmate_types::McpRemoteToolSummary;

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
    /// 最近打开的工作区根（新在前；与 `last_workspace_root` 同步为首项）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_workspace_roots: Vec<String>,
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

/// `mcp_servers.json` 单条 MCP 服务器（用户数据目录，非 TOML）。
/// stdio（`command`）或远程 Streamable HTTP（`url`）；二者勿同时用于同一条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntry {
    pub id: String,
    pub name: String,
    pub slug: String,
    /// 可执行文件，或 legacy 整行命令（`args`/`env`/`cwd` 皆空时按词法拆分启动）。
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Streamable HTTP MCP 端点（与非空 `command` 互斥）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 远程请求头（如 `Authorization`）；GET 公开体仅暴露 `has_headers`。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl McpServerEntry {
    pub fn has_stdio(&self) -> bool {
        !self.command.trim().is_empty()
    }

    pub fn has_remote_url(&self) -> bool {
        self.url.as_ref().is_some_and(|u| !u.trim().is_empty())
    }
}

/// Web `GET /user-data/mcp-servers`：不返回启动命令/URL/头明文（仅 `has_*` 标志）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerEntryPublic {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    pub has_command: bool,
    #[serde(default)]
    pub has_args: bool,
    #[serde(default)]
    pub has_env: bool,
    #[serde(default)]
    pub has_cwd: bool,
    #[serde(default)]
    pub has_url: bool,
    #[serde(default)]
    pub has_headers: bool,
    /// 本机 `secrets/mcp_bearer_{id}` 是否已设置（不回传明文）。
    #[serde(default)]
    pub has_bearer: bool,
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

impl McpServersFilePublic {
    /// 构造公开体；`has_bearer` 由调用方按 server id 查询 secrets。
    pub fn from_file_with_bearer<F>(file: &McpServersFile, mut bearer_set: F) -> Self
    where
        F: FnMut(&str) -> bool,
    {
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
                    has_command: s.has_stdio(),
                    has_args: !s.args.is_empty(),
                    has_env: !s.env.is_empty(),
                    has_cwd: s.cwd.as_ref().is_some_and(|c| !c.trim().is_empty()),
                    has_url: s.has_remote_url(),
                    has_headers: !s.headers.is_empty(),
                    has_bearer: bearer_set(&s.id),
                    created_at_ms: s.created_at_ms,
                    updated_at_ms: s.updated_at_ms,
                })
                .collect(),
        }
    }
}

impl From<&McpServersFile> for McpServersFilePublic {
    fn from(file: &McpServersFile) -> Self {
        Self::from_file_with_bearer(file, |_| false)
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
pub struct McpServerStatusEntry {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub enabled: bool,
    pub connected: bool,
    /// `stdio` | `remote` | `none`
    #[serde(default)]
    pub transport: String,
    pub openai_tool_names: Vec<String>,
    pub remote_tools: Vec<McpRemoteToolSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    /// 连接失败分类（如 `dns` / `tls` / `unauthorized` / `handshake`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error_kind: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServersStatusResponse {
    pub global_enabled: bool,
    pub tool_timeout_secs: u64,
    pub servers: Vec<McpServerStatusEntry>,
}
