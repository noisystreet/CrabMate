//! GitHub 在线模式 API（需 Bearer 鉴权，与 `/workspace` 同级）。

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::AppState;
use crate::web::github::{
    github_oauth_device_cancel_handler, github_oauth_device_logout_handler,
    github_oauth_device_start_handler, github_oauth_device_status_handler,
    github_pr_current_checks_handler, github_repo_context_handler,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/github/repo-context", get(github_repo_context_handler))
        .route(
            "/github/pr/current/checks",
            get(github_pr_current_checks_handler),
        )
        .route(
            "/github/oauth/device/start",
            post(github_oauth_device_start_handler),
        )
        .route(
            "/github/oauth/device/status",
            get(github_oauth_device_status_handler),
        )
        .route(
            "/github/oauth/device/cancel",
            post(github_oauth_device_cancel_handler),
        )
        .route(
            "/github/oauth/device/logout",
            post(github_oauth_device_logout_handler),
        )
}
