//! `GET` / `POST /tasks` 路由；JSON 见 [`crate::cm_tools::workspace::tasks_side`]。

use std::sync::Arc;

use axum::{Router, routing::get};

use crate::AppState;
use crate::web::task::{tasks_get_handler, tasks_set_handler};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new().route("/tasks", get(tasks_get_handler).post(tasks_set_handler))
}
