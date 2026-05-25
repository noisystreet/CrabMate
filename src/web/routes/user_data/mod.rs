//! `/user-data/*` 路由表。

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, put},
};

use crate::AppState;
use crate::web::user_data::{
    get_current_sessions_handler, get_llm_overrides_handler, get_prefs_handler,
    get_secrets_status_handler, get_workspaces_handler, put_current_sessions_handler,
    put_llm_overrides_handler, put_prefs_handler, put_secret_client_llm_handler,
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
}
