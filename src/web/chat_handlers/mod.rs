//! `/chat`、`/upload`、`/health`、`/status` 等 Axum handler（自 `lib.rs` 下沉）。
//!
//! 本模块为 `web` 私有子模块；路由表在 [`crate::web::routes`] 中 `merge` 各域 `router()`，handler 由此再导出。
//!
//! | 子模块 | 职责 |
//! |--------|------|
//! | JSON 体 | [`crate::web::http_types::chat`] 与 [`crate::web::http_types::workspace`]（changelog） |
//! | [`parse`] | conversation_id、client_llm、seed、temperature 等校验 |
//! | [`conflict`] | 会话 revision 冲突的 HTTP/SSE 载荷 |
//! | [`upload`] | 上传与 uploads 目录清理 |
//! | [`auth`] | Web API Bearer 中间件 |
//! | [`chat`] | `/chat*` 与队列入队 |
//! | [`workspace_changelog`] | `GET /workspace/changelog` |
//! | [`health_status`] | `GET /health`、`GET /status`、`GET /web-ui` |
//! | [`config_reload`] | `POST /config/reload` |

mod auth;
mod chat;
mod config_reload;
mod conflict;
mod health_status;
mod parse;
mod upload;
mod workspace_changelog;

pub(crate) use auth::require_web_api_bearer_auth;
pub(crate) use chat::{
    chat_approval_handler, chat_branch_handler, chat_handler, chat_stream_handler,
};
pub(crate) use config_reload::config_reload_handler;
pub(crate) use conflict::conversation_conflict_sse_line;
pub(crate) use health_status::{health_handler, status_handler, web_ui_config_handler};
#[cfg(test)]
pub(crate) use parse::normalize_client_conversation_id;
pub(crate) use upload::{cleanup_uploads_dir, delete_uploads_handler, upload_handler};
pub(crate) use workspace_changelog::workspace_changelog_handler;
