//! `GET` / `POST /tasks` JSON 形状；路由见 [`crate::web::routes::tasks::router`]。
//!
//! 类型定义在 **`crabmate-web-host`**；存储位于 [`crate::process_handles::ProcessHandles::workspace_tasks_by_path`]。

pub use crabmate_web_host::http_types::tasks::TasksData;
