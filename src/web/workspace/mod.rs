//! 工作区浏览、文件读写、搜索、画像等 HTTP handler；JSON 形状见 [`crate::web::http_types::workspace`]（路由表见 [`crate::web::routes::workspace::router`]）。

mod handlers;

pub use handlers::*;
