//! `GET` / `POST /tasks` JSON 形状；路由见 [`crate::web::routes::tasks::router`]。
//!
//! 存储位于 [`crate::process_handles::ProcessHandles::workspace_tasks_by_path`]（进程内存）。

pub use crate::workspace::tasks_side::TasksData;
