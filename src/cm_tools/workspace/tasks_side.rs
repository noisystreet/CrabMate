//! 进程内任务清单（与 Web `GET`/`POST /tasks` 同源）；键为规范化工作区路径字符串。

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub done: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct TasksData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub items: Vec<TaskItem>,
}

pub type WorkspaceTasksByPath = Arc<RwLock<HashMap<String, TasksData>>>;
