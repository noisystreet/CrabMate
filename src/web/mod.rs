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

pub(crate) use app_state::{
    AppState, AppStateChatRuntime, AppStateConversationRuntime, AppStateHttpCore, AppStateWebAux,
    ConversationBacking, WebChatJobAppFacet, open_conversation_sqlite,
};
pub(crate) use chat_handlers::{cleanup_uploads_dir, conversation_conflict_sse_line};
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
pub(crate) mod skills_handlers;
pub mod task;
pub(crate) mod tool_jobs;
mod user_data;
pub mod workspace;
