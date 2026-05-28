//! 本机用户数据 HTTP 面（`docs/design/user_data_dir.md` §7）。

mod handlers;

pub(crate) use handlers::{
    get_current_sessions_handler, get_llm_overrides_handler, get_mcp_servers_handler,
    get_mcp_servers_status_handler, get_prefs_handler, get_secrets_status_handler,
    get_workspaces_handler, post_mcp_server_probe_handler, post_mcp_servers_import_handler,
    post_mcp_servers_probe_all_handler, put_current_sessions_handler, put_llm_overrides_handler,
    put_mcp_servers_handler, put_prefs_handler, put_secret_client_llm_handler,
    put_secret_executor_llm_handler, put_secret_web_api_bearer_handler,
};
