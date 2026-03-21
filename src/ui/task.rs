use std::path::Path;
use std::sync::Arc;

use axum::{Json, extract::State};
use serde::{Deserialize, Serialize};
use tracing::error;

use crate::AppState;

#[derive(Serialize, Deserialize, Clone)]
pub struct TaskItem {
    pub id: String,
    pub title: String,
    pub done: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TasksData {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub items: Vec<TaskItem>,
}

/// 读取当前工作区根目录下的 tasks.json；若不存在则返回空任务列表
pub async fn tasks_get_handler(State(state): State<Arc<AppState>>) -> Json<TasksData> {
    let base_str = state.effective_workspace_path().await;
    let root = Path::new(&base_str);
    let path = root.join("tasks.json");
    if !path.exists() {
        return Json(TasksData {
            source: None,
            updated_at: None,
            items: Vec::new(),
        });
    }
    match tokio::fs::read_to_string(&path).await {
        Ok(s) => match serde_json::from_str::<TasksData>(&s) {
            Ok(data) => Json(data),
            Err(e) => {
                error!(error = %e, "解析 tasks.json 失败，将返回空任务列表");
                Json(TasksData {
                    source: None,
                    updated_at: None,
                    items: Vec::new(),
                })
            }
        },
        Err(e) => {
            error!(error = %e, "读取 tasks.json 失败，将返回空任务列表");
            Json(TasksData {
                source: None,
                updated_at: None,
                items: Vec::new(),
            })
        }
    }
}

/// 覆盖写入当前工作区根目录的 tasks.json
pub async fn tasks_set_handler(
    State(state): State<Arc<AppState>>,
    Json(mut body): Json<TasksData>,
) -> Json<TasksData> {
    let base_str = state.effective_workspace_path().await;
    let root = Path::new(&base_str);
    let path = root.join("tasks.json");
    // 由后端统一维护更新时间
    let now = chrono::Utc::now().to_rfc3339();
    body.updated_at = Some(now);
    let content = match serde_json::to_string_pretty(&body) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "序列化任务数据失败");
            return Json(body);
        }
    };
    if let Some(parent) = path.parent()
        && let Err(e) = tokio::fs::create_dir_all(parent).await
    {
        error!(error = %e, "创建 tasks.json 目录失败");
    }
    if let Err(e) = tokio::fs::write(&path, content.as_bytes()).await {
        error!(error = %e, "写入 tasks.json 失败");
    }
    Json(body)
}
