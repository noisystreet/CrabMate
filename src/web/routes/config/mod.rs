//! `POST /config/reload`（与对话域分离，便于配置契约单独对照文档）。

use std::sync::Arc;

use axum::{Router, routing::post};

use crate::AppState;
use crate::web::chat_handlers::config_reload_handler;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new().route("/config/reload", post(config_reload_handler))
}
