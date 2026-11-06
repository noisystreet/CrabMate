//! 本机用户数据目录（`~/.local/share/crabmate`）：prefs、按工作区 Web 会话、LLM 覆盖与
//! secrets；机密（Web API Bearer / MCP Bearer）在系统钥匙串。
//!
//! 设计说明：**`docs/design/user_data_dir.md`**。
//!
//! 本模块是**本体**（S4-P1b，见 #1300）：磁盘读写与类型定义原在 `cm_internal::user_data`，
//! 经 `pub use …::*` 转发到此处；现已并入本模块并删除转发行。Web 请求体合并
//! （依赖 `web::http_types`）一直就在根 crate。

mod cli_llm_align;
mod credential_store;
mod io;
pub mod mcp_json_import;
mod mcp_slug;
mod merge;
mod path;
mod saved_model_secrets;
mod store;
mod types;

pub use cli_llm_align::{
    apply_user_data_llm_overrides, apply_user_data_tool_overrides, apply_user_data_web_api_bearer,
};
pub use merge::{merge_client_llm_body, merge_executor_llm_body};
pub use path::user_data_root;
pub use store::{
    append_mcp_json_import, ensure_user_data_tree, list_workspaces, load_llm_overrides,
    load_mcp_servers, load_meta, load_prefs, load_tool_overrides, load_web_sessions,
    mcp_servers_file_public, merge_mcp_commands_from_stored,
    normalize_mcp_servers_file, read_secret_mcp_bearer, read_secret_web_api_bearer,
    save_llm_overrides, save_mcp_servers, save_prefs, save_tool_overrides, save_web_sessions,
    secrets_status, validate_mcp_secret_server_id, validate_sessions_value,
    write_secret_mcp_bearer, write_secret_web_api_bearer,
};
pub use types::{
    LlmEndpointOverride, LlmOverridesFile, McpServerStatusEntry, McpServersFile,
    McpServersFilePublic, McpServersImportResponse, McpServersStatusResponse, SCHEMA_VERSION,
    SecretsStatusResponse, UserPrefs, WebSessionsFile, WorkspaceListEntry,
};
