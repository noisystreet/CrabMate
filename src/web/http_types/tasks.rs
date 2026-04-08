//! `GET` / `POST /tasks` JSON 形状；路由见 [`crate::web::routes::tasks::router`]；数据仅存进程内存，见 [`crate::web::app_state::AppState::web_tasks_by_workspace`]。

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub done: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct TasksData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub items: Vec<TaskItem>,
}
