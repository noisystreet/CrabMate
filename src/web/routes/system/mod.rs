//! `GET /health`、`GET /status`（JSON 形状在各自 handler 模块内定义）。

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::AppState;
use crate::web::chat_handlers::{health_handler, status_handler, web_ui_config_handler};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/health", get(health_handler))
        .route("/status", get(status_handler))
        .route("/web-ui", get(web_ui_config_handler))
}
