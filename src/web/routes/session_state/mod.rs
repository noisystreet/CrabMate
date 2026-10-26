//! `POST /conversation/state`：会话级运行态意图的写入口（handler 在 [`crate::web::session_state`]）。
//!
//! 挂**受保护** API 面（与 `GET /permission` 同面）：它写的是授权相关的运行态意图。

use std::sync::Arc;

use axum::{Router, routing::post};

use crate::AppState;
use crate::web::session_state::conversation_state_handler;

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new().route(
        "/conversation/state",
        post(conversation_state_handler),
    )
}
