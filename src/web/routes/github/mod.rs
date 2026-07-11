//! GitHub 在线模式只读 API（需 Bearer 鉴权，与 `/workspace` 同级）。

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use crate::AppState;
use crate::web::github::{
    github_pr_current_checks_handler, github_prs_handler, github_repo_context_handler,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/github/repo-context", get(github_repo_context_handler))
        .route("/github/prs", get(github_prs_handler))
        .route(
            "/github/pr/current/checks",
            get(github_pr_current_checks_handler),
        )
}
