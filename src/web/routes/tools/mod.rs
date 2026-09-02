//! `GET /tools/jobs/{tool_job_id}`、`GET /tools/jobs/{tool_job_id}/output`、
//! `POST /tools/jobs/{tool_job_id}/cancel` 路由；
//! JSON 见 [`crate::web::http_types::tool_jobs`]，handler 见 [`crate::web::tool_jobs`]。

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::AppState;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/tools/jobs/{tool_job_id}",
            get(crate::web::tool_jobs::tool_job_status_handler),
        )
        .route(
            "/tools/jobs/{tool_job_id}/output",
            get(crate::web::tool_jobs::tool_job_output_handler),
        )
        .route(
            "/tools/jobs/{tool_job_id}/cancel",
            post(crate::web::tool_jobs::tool_job_cancel_handler),
        )
}
