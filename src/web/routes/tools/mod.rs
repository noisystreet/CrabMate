//! `GET /tools`、`GET /tools/plugins`、`GET /tools/{tool_name}`、
//! `PUT /tools/{tool_name}/enabled`、`GET /tools/jobs/{tool_job_id}`、
//! `GET /tools/jobs/{tool_job_id}/output`、`POST /tools/jobs/{tool_job_id}/cancel` 路由；
//! JSON 见 [`crate::web::http_types::tools`] / [`crate::web::http_types::tool_jobs`]，
//! handler 见 [`crate::web::tools_handlers`] / [`crate::web::tool_jobs`]。

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post, put};

use crate::AppState;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/tools", get(crate::web::tools_handlers::tools_list_handler))
        .route(
            "/tools/plugins",
            get(crate::web::tools_handlers::tools_plugins_handler),
        )
        .route(
            "/tools/{tool_name}",
            get(crate::web::tools_handlers::tool_detail_handler),
        )
        .route(
            "/tools/{tool_name}/enabled",
            put(crate::web::tools_handlers::tool_enabled_handler),
        )
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
