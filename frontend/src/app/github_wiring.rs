//! GitHub 在线模式：`/github/*` 拉取与侧栏可见时刷新。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    GithubPrCurrentChecksData, GithubPrsData, fetch_github_pr_current_checks, fetch_github_prs,
    fetch_github_repo_context,
};
use crate::app_prefs::SidePanelView;
use crate::i18n::Locale;

use super::status_tasks_state::StatusTasksSignals;

pub fn make_refresh_github(st: StatusTasksSignals, locale: Locale) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        st.github_loading.set(true);
        st.github_err.set(None);
        spawn_local(async move {
            let repo = fetch_github_repo_context(locale).await;
            let prs = fetch_github_prs(locale).await;
            let checks = fetch_github_pr_current_checks(locale).await;

            match repo {
                Ok(d) => {
                    if let Some(e) = d.error.clone().filter(|s| !s.trim().is_empty()) {
                        st.github_err.set(Some(e));
                    }
                    st.github_repo.set(Some(d));
                }
                Err(e) => {
                    st.github_repo.set(None);
                    st.github_err.set(Some(e));
                }
            }

            match prs {
                Ok(d) => {
                    if let Some(e) = d.error.clone().filter(|s| !s.trim().is_empty()) {
                        st.github_err.set(Some(e));
                    }
                    st.github_prs.set(d);
                }
                Err(e) => {
                    st.github_prs.set(GithubPrsData {
                        items: vec![],
                        error: Some(e.clone()),
                    });
                    st.github_err.set(Some(e));
                }
            }

            match checks {
                Ok(d) => {
                    if let Some(e) = d.error.clone().filter(|s| !s.trim().is_empty()) {
                        st.github_err.set(Some(e));
                    }
                    st.github_checks.set(Some(d));
                }
                Err(e) => {
                    st.github_checks.set(Some(GithubPrCurrentChecksData {
                        error: Some(e.clone()),
                        ..Default::default()
                    }));
                    st.github_err.set(Some(e));
                }
            }

            st.github_loading.set(false);
        });
    })
}

/// 仅刷新状态栏 PR 摘要（仓库上下文 + 当前分支 checks）。
pub fn make_refresh_github_status_chip(
    st: StatusTasksSignals,
    locale: Locale,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        spawn_local(async move {
            if let Ok(d) = fetch_github_repo_context(locale).await {
                if d.error.as_deref().unwrap_or("").is_empty() {
                    st.github_repo.set(Some(d));
                }
            }
            if let Ok(d) = fetch_github_pr_current_checks(locale).await {
                if d.error.as_deref().unwrap_or("").is_empty() || d.pr_number.is_some() {
                    st.github_checks.set(Some(d));
                }
            }
        });
    })
}

pub fn wire_github_refresh_when_panel_visible(
    side_panel_view: RwSignal<SidePanelView>,
    initialized: RwSignal<bool>,
    refresh_github: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_github = Arc::clone(&refresh_github);
        move |_| {
            if matches!(side_panel_view.get(), SidePanelView::PullRequests) && initialized.get() {
                refresh_github();
            }
        }
    });
}

pub fn wire_github_status_chip_after_init(
    initialized: RwSignal<bool>,
    refresh_github_status: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_github_status = Arc::clone(&refresh_github_status);
        move |_| {
            if initialized.get() {
                refresh_github_status();
            }
        }
    });
}
