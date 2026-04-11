//! `GET /status` 与侧栏任务清单（`/tasks`）相关的 **`RwSignal`** 聚合。
//!
//! 与 [`crate::chat_session_state::ChatSessionSignals`]、[`super::workspace_panel_state::WorkspacePanelSignals`]
//! 并列，减少 `App` 向底栏、右栏逐项传参。

use leptos::prelude::*;

use crate::api::{StatusData, TasksData};

/// 服务端状态快照 + 任务列表的响应式句柄（不含「流式对话错误」`status_err`，仍单独传递）。
#[derive(Clone, Copy)]
pub struct StatusTasksSignals {
    pub status_data: RwSignal<Option<StatusData>>,
    pub status_loading: RwSignal<bool>,
    pub status_fetch_err: RwSignal<Option<String>>,
    pub tasks_data: RwSignal<TasksData>,
    pub tasks_err: RwSignal<Option<String>>,
    pub tasks_loading: RwSignal<bool>,
}
