//! 工作区浏览、文件读写、搜索、画像等 HTTP handler；JSON 形状见 [`crate::web::http_types::workspace`]（路由表见 [`crate::web::routes::workspace::router`]）。

mod clone_stream;
mod clone_validate;
mod handlers;
mod handlers_dir_archive;
#[cfg(feature = "archive-tools")]
mod handlers_dir_archive_zip;
mod handlers_dir_tests;
mod handlers_file_download;
mod handlers_file_raw;
mod handlers_file_move;
mod handlers_file_raw_put;
#[cfg(test)]
mod handlers_file_raw_http_tests;
#[cfg(test)]
mod handlers_fs_extra_http_tests;
mod handlers_sync;
mod projects;

pub use clone_stream::workspace_clone_stream_handler;
pub use handlers::*;
pub use handlers_dir_archive::workspace_dir_archive_handler;
pub use handlers_file_download::workspace_file_download_handler;
pub use handlers_file_move::workspace_file_move_handler;
pub use handlers_file_raw::workspace_file_raw_handler;
pub use handlers_file_raw_put::workspace_file_raw_put_handler;
pub use projects::{workspace_projects_list_handler, workspace_projects_post_handler};
