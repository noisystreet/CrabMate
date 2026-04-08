use std::sync::Arc;

use axum::{Json, extract::State};

use crate::AppState;

use super::http_types::tasks::TasksData;

/// 读取当前工作区对应的任务清单（**进程内存**；服务重启后清空；切换工作区则各键独立）。
pub async fn tasks_get_handler(State(state): State<Arc<AppState>>) -> Json<TasksData> {
    let key = state.effective_workspace_path().await;
    let guard = state.web_tasks_by_workspace.read().await;
    Json(guard.get(&key).cloned().unwrap_or_default())
}

/// 覆盖保存当前工作区的任务清单（**仅内存**；不创建或修改工作区内任何文件）。
pub async fn tasks_set_handler(
    State(state): State<Arc<AppState>>,
    Json(mut body): Json<TasksData>,
) -> Json<TasksData> {
    let key = state.effective_workspace_path().await;
    body.updated_at = Some(chrono::Utc::now().to_rfc3339());
    let mut guard = state.web_tasks_by_workspace.write().await;
    guard.insert(key, body.clone());
    Json(body)
}
