//! `GET /health`、`GET /status`（JSON 形状在各自 handler 模块内定义）；`GET /web-ui` 在 **`crabmate-web-host`**。

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::AppState;
use crate::web::chat_handlers::{health_handler, status_handler};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .merge(crabmate_web_host::routes::web_ui::router())
}
