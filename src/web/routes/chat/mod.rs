//! 对话、上传等与 `chat_handlers` 对应的路由表。
//!
//! JSON 形状见 [`crate::web::http_types::chat`]；实现见 [`crate::web::chat_handlers`]。

use std::sync::Arc;

use axum::{Router, routing::post};

use crate::AppState;
use crate::web::chat_handlers::{
    chat_approval_handler, chat_branch_handler, chat_handler, chat_stream_handler,
    delete_uploads_handler, upload_handler,
};

pub(crate) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/chat", post(chat_handler))
        .route("/chat/stream", post(chat_stream_handler))
        .route("/chat/approval", post(chat_approval_handler))
        .route("/chat/branch", post(chat_branch_handler))
        .route("/upload", post(upload_handler))
        .route("/uploads/delete", post(delete_uploads_handler))
}
