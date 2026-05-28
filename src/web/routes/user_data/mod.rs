//! `/user-data/*` 路由表。

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post, put},
};

use crate::AppState;
use crate::web::user_data::{
    get_current_sessions_handler, get_llm_overrides_handler, get_mcp_servers_handler,
    get_mcp_servers_status_handler, get_prefs_handler, get_secrets_status_handler,
    get_workspaces_handler, post_mcp_server_probe_handler, post_mcp_servers_import_handler,
    post_mcp_servers_probe_all_handler, put_current_sessions_handler, put_llm_overrides_handler,
    put_mcp_servers_handler, put_prefs_handler, put_secret_client_llm_handler,
    put_secret_executor_llm_handler, put_secret_web_api_bearer_handler,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/user-data/prefs",
            get(get_prefs_handler).put(put_prefs_handler),
        )
        .route(
            "/user-data/llm-overrides",
            get(get_llm_overrides_handler).put(put_llm_overrides_handler),
        )
        .route("/user-data/secrets/status", get(get_secrets_status_handler))
        .route(
            "/user-data/secrets/client-llm",
            put(put_secret_client_llm_handler),
        )
        .route(
            "/user-data/secrets/executor-llm",
            put(put_secret_executor_llm_handler),
        )
        .route(
            "/user-data/secrets/web-api-bearer",
            put(put_secret_web_api_bearer_handler),
        )
        .route("/user-data/workspaces", get(get_workspaces_handler))
        .route(
            "/user-data/workspaces/current/sessions",
            get(get_current_sessions_handler).put(put_current_sessions_handler),
        )
        .route(
            "/user-data/mcp-servers",
            get(get_mcp_servers_handler).put(put_mcp_servers_handler),
        )
        .route(
            "/user-data/mcp-servers/import",
            post(post_mcp_servers_import_handler),
        )
        .route(
            "/user-data/mcp-servers/status",
            get(get_mcp_servers_status_handler),
        )
        .route(
            "/user-data/mcp-servers/probe-all",
            post(post_mcp_servers_probe_all_handler),
        )
        .route(
            "/user-data/mcp-servers/{id}/probe",
            post(post_mcp_server_probe_handler),
        )
}
