//! `GET` / `POST /tasks` JSON 形状；路由见 [`crate::web::routes::tasks::router`]。
//!
//! 类型定义在 [`crate::cm_tools::workspace::tasks_side::TasksData`]；存储位于
//! [`crate::process_handles::ProcessHandles::workspace_tasks_by_path`]。

pub use crate::cm_tools::workspace::tasks_side::TasksData;
