//! 工作区浏览、文件读写、搜索、画像等 HTTP handler；JSON 形状见 [`crate::web::http_types::workspace`]（路由表见 [`crate::web::routes::workspace::router`]）。

mod clone_stream;
mod clone_validate;
mod handlers;
mod handlers_dir_tests;
mod handlers_file_raw;
mod handlers_sync;
mod projects;

pub use clone_stream::workspace_clone_stream_handler;
pub use handlers::*;
pub use handlers_file_raw::workspace_file_raw_handler;
pub use projects::{workspace_projects_list_handler, workspace_projects_post_handler};
