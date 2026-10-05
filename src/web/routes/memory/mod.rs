//! `/memory/*` 路由表。
//!
//! JSON 形状见 [`crate::web::http_types::memory`]；实现见 [`crate::web::memory`]。

use std::sync::Arc;

use axum::{
    Router,
    routing::{delete, get},
};

use crate::AppState;
use crate::web::memory::{memory_delete_handler, memory_list_handler};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/memory/list", get(memory_list_handler))
        .route("/memory/{id}", delete(memory_delete_handler))
}
