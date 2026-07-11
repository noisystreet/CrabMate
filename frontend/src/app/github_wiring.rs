//! GitHub 在线模式：仓库上下文（侧栏 GitHub 按钮）。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{WorkspaceData, fetch_github_repo_context};
use crate::i18n::Locale;

use super::status_tasks_state::StatusTasksSignals;

/// 刷新侧栏 GitHub 仓库按钮上下文。
pub fn make_refresh_github_repo_context(
    st: StatusTasksSignals,
    locale: Locale,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        spawn_local(async move {
            match fetch_github_repo_context(locale).await {
                Ok(d) => st.github_repo.set(Some(d)),
                Err(_) => st.github_repo.set(None),
            }
        });
    })
}

pub fn wire_github_repo_after_init(
    initialized: RwSignal<bool>,
    refresh_github_repo: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_github_repo = Arc::clone(&refresh_github_repo);
        move |_| {
            if initialized.get() {
                refresh_github_repo();
            }
        }
    });
}

/// 工作区路径变更后重新拉取 GitHub 上下文（`POST /workspace/set` 后 `workspace_data` 会更新）。
pub fn wire_github_refresh_when_workspace_changes(
    workspace_data: RwSignal<Option<WorkspaceData>>,
    initialized: RwSignal<bool>,
    refresh_github_repo: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_github_repo = Arc::clone(&refresh_github_repo);
        move |_| {
            if !initialized.get() {
                return;
            }
            let Some(d) = workspace_data.get() else {
                return;
            };
            if d.path.trim().is_empty() {
                return;
            }
            refresh_github_repo();
        }
    });
}
