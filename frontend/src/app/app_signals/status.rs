//! `/status` 与 `/tasks` 列表快照。

use leptos::prelude::*;

use crate::api::{
    GithubPrCurrentChecksData, GithubPrsData, GithubRepoContextData, StatusData, TasksData,
};

use crate::app::status_fetch_state::StatusFetchPhase;

#[derive(Clone, Copy)]
pub struct StatusSignals {
    pub status_data: RwSignal<Option<StatusData>>,
    pub status_loading: RwSignal<bool>,
    pub status_fetch_phase: RwSignal<StatusFetchPhase>,
    pub status_fetch_err: RwSignal<Option<String>>,
    pub tasks_data: RwSignal<TasksData>,
    pub tasks_err: RwSignal<Option<String>>,
    pub tasks_loading: RwSignal<bool>,
    pub github_repo: RwSignal<Option<GithubRepoContextData>>,
    pub github_prs: RwSignal<GithubPrsData>,
    pub github_checks: RwSignal<Option<GithubPrCurrentChecksData>>,
    pub github_loading: RwSignal<bool>,
    pub github_err: RwSignal<Option<String>>,
}

impl StatusSignals {
    pub fn new() -> Self {
        Self {
            status_data: RwSignal::new(None),
            status_loading: RwSignal::new(false),
            status_fetch_phase: RwSignal::new(StatusFetchPhase::Idle),
            status_fetch_err: RwSignal::new(None),
            tasks_data: RwSignal::new(TasksData { items: vec![] }),
            tasks_err: RwSignal::new(None),
            tasks_loading: RwSignal::new(false),
            github_repo: RwSignal::new(None),
            github_prs: RwSignal::new(GithubPrsData {
                items: vec![],
                error: None,
            }),
            github_checks: RwSignal::new(None),
            github_loading: RwSignal::new(false),
            github_err: RwSignal::new(None),
        }
    }
}

impl Default for StatusSignals {
    fn default() -> Self {
        Self::new()
    }
}
