//! `/status` 与 `/tasks` 列表快照。

use leptos::prelude::*;

use crate::api::{StatusData, TasksData};

#[derive(Clone, Copy)]
pub struct StatusSignals {
    pub status_data: RwSignal<Option<StatusData>>,
    pub status_loading: RwSignal<bool>,
    pub status_fetch_err: RwSignal<Option<String>>,
    pub tasks_data: RwSignal<TasksData>,
    pub tasks_err: RwSignal<Option<String>>,
    pub tasks_loading: RwSignal<bool>,
}

impl StatusSignals {
    pub fn new() -> Self {
        Self {
            status_data: RwSignal::new(None),
            status_loading: RwSignal::new(true),
            status_fetch_err: RwSignal::new(None),
            tasks_data: RwSignal::new(TasksData { items: vec![] }),
            tasks_err: RwSignal::new(None),
            tasks_loading: RwSignal::new(false),
        }
    }
}

impl Default for StatusSignals {
    fn default() -> Self {
        Self::new()
    }
}
