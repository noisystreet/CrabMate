//! 本机用户数据目录（`~/.local/share/crabmate`）：prefs、按工作区 Web 会话、LLM 覆盖与 secrets。
//!
//! 设计说明：**`docs/design/user_data_dir.md`**。

mod io;
pub mod mcp_json_import;
mod mcp_slug;
mod path;
mod store;
mod types;

pub use path::user_data_root;
pub use store::{
    append_mcp_json_import, ensure_user_data_tree, list_workspaces, load_llm_overrides,
    load_mcp_servers_with_legacy_import, load_meta, load_prefs, load_web_sessions,
    merge_mcp_commands_from_stored, normalize_mcp_servers_file, read_secret_client_llm,
    read_secret_executor_llm, save_llm_overrides, save_mcp_servers, save_prefs, save_web_sessions,
    secrets_status, validate_sessions_value, write_secret_client_llm, write_secret_executor_llm,
    write_secret_web_api_bearer,
};
pub use types::{
    LlmEndpointOverride, LlmOverridesFile, McpRemoteToolSummary, McpServerStatusEntry,
    McpServersFile, McpServersFilePublic, McpServersImportResponse, McpServersStatusResponse,
    SCHEMA_VERSION, SecretsStatusResponse, UserPrefs, WebSessionsFile, WorkspaceListEntry,
};
