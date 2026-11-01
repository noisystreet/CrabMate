//! Axum handler：浏览器 Web UI / Client 调用的工作区、任务等 HTTP API。
mod app_state;
mod app_state_facets;
mod async_chat_job;
pub(crate) mod audit;
mod chat_handlers;
mod chat_uploads_paths;
mod conversation_messages_window;
pub(crate) mod cron_scheduler;
pub(crate) mod http_types;

/// Web JSON 错误响应体：`{ "ok": false, "code": ..., "error": ... }`。
///
/// `#1247`：`github::device_flow::err_json` 与 `workspace::clone_stream::clone_http_err`
/// 曾是两份字节级相同的实现，收敛至此。两处都保留原名 `use` 别名，调用点未动。
pub(crate) fn json_error_response(
    status: axum::http::StatusCode,
    code: &str,
    message: &str,
) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    (
        status,
        axum::Json(serde_json::json!({
            "ok": false,
            "code": code,
            "error": message,
        })),
    )
}
mod memory;
mod permission;

pub(crate) use app_state::{
    AppState, AppStateChatRuntime, AppStateConversationRuntime, AppStateHttpCore, AppStateWebAux,
    ConversationBacking, WebChatJobAppFacet, open_conversation_sqlite,
};
pub(crate) use chat_handlers::{cleanup_uploads_dir, conversation_conflict_sse_line};
pub(crate) use chat_handlers::spawn_tiktoken_baseline_warmup;
pub(crate) use chat_uploads_paths::{
    chat_uploads_dir_beside_session_store, sync_chat_runtime_paths_for_workspace,
};

pub(crate) use chat_handlers::normalize_client_conversation_id;

pub mod github;
pub(crate) mod github_token_request;
pub mod openapi;
pub(crate) mod request_id;
pub mod routes;
pub mod server;
mod session_state;
pub(crate) mod skills_handlers;
pub mod task;
pub(crate) mod tool_jobs;
pub(crate) mod tools_handlers;
mod user_data;
pub mod workspace;
