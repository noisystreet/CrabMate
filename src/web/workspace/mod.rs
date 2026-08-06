//! 工作区浏览、文件读写、搜索、画像等 HTTP handler；JSON 形状见 [`crate::web::http_types::workspace`]（路由表见 [`crate::web::routes::workspace::router`]）。

mod handlers;
mod handlers_dir_tests;
mod handlers_sync;
mod projects;

pub use handlers::*;
pub use projects::{workspace_projects_list_handler, workspace_projects_post_handler};
