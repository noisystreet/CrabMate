//! `/workspace*` 路由表；handler 在 [`crate::web::workspace`]，JSON 在 [`crate::web::http_types::workspace`]。

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};

use crate::AppState;
use crate::web::chat_handlers::workspace_changelog_handler;
use crate::web::workspace::{
    workspace_file_delete_handler, workspace_file_read_handler, workspace_file_write_handler,
    workspace_handler, workspace_pick_handler, workspace_profile_handler, workspace_search_handler,
    workspace_set_handler,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/workspace",
            get(workspace_handler).post(workspace_set_handler),
        )
        .route("/workspace/pick", get(workspace_pick_handler))
        .route("/workspace/search", post(workspace_search_handler))
        .route(
            "/workspace/file",
            get(workspace_file_read_handler)
                .post(workspace_file_write_handler)
                .delete(workspace_file_delete_handler),
        )
        .route("/workspace/profile", get(workspace_profile_handler))
        .route("/workspace/changelog", get(workspace_changelog_handler))
}
