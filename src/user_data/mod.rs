//! 本机用户数据目录（`~/.local/share/crabmate`）：prefs、按工作区 Web 会话、LLM 覆盖与 secrets。
//!
//! 设计说明：**`docs/design/user_data_dir.md`**。

mod io;
mod merge;
mod path;
mod store;
mod types;

pub use merge::{merge_client_llm_body, merge_executor_llm_body};

pub use path::user_data_root;
pub use store::{
    ensure_user_data_tree, list_workspaces, load_llm_overrides, load_meta, load_prefs,
    load_web_sessions, save_llm_overrides, save_prefs, save_web_sessions, secrets_status,
    validate_sessions_value, write_secret_client_llm, write_secret_executor_llm,
    write_secret_web_api_bearer,
};
pub use types::{
    LlmOverridesFile, SCHEMA_VERSION, SecretsStatusResponse, UserPrefs, WebSessionsFile,
    WorkspaceListEntry,
};
