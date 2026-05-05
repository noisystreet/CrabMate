//! `/status`、`/tasks` 拉取与侧栏任务面可见时刷新的 **`Effect`** / 闭包工厂。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{fetch_status, fetch_tasks, save_tasks};
use crate::app_prefs::SidePanelView;
use crate::i18n::Locale;

use super::status_tasks_state::StatusTasksSignals;

pub fn make_refresh_status(
    st: StatusTasksSignals,
    selected_agent_role: RwSignal<Option<String>>,
    locale: Locale,
) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        st.status_loading.set(true);
        st.status_fetch_err.set(None);
        spawn_local(async move {
            match fetch_status(locale).await {
                Ok(d) => {
                    st.status_fetch_err.set(None);
                    if let Some(cur) = selected_agent_role.get_untracked()
                        && !d.agent_role_ids.iter().any(|id| id == &cur)
                    {
                        selected_agent_role.set(None);
                    }
                    st.status_data.set(Some(d));
                }
                Err(e) => {
                    st.status_data.set(None);
                    st.status_fetch_err.set(Some(e));
                }
            }
            st.status_loading.set(false);
        });
    })
}

pub fn make_refresh_tasks(st: StatusTasksSignals, locale: Locale) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        st.tasks_loading.set(true);
        spawn_local(async move {
            match fetch_tasks(locale).await {
                Ok(d) => {
                    st.tasks_err.set(None);
                    st.tasks_data.set(d);
                }
                Err(e) => {
                    st.tasks_err.set(Some(e));
                }
            }
            st.tasks_loading.set(false);
        });
    })
}

pub fn make_toggle_task(
    st: StatusTasksSignals,
    locale: Locale,
) -> Arc<dyn Fn(String) + Send + Sync> {
    Arc::new(move |id: String| {
        let mut next = st.tasks_data.get();
        if let Some(i) = next.items.iter().position(|t| t.id == id) {
            next.items[i].done = !next.items[i].done;
            let n = next.clone();
            let td = st.tasks_data;
            spawn_local(async move {
                if let Ok(saved) = save_tasks(&n, locale).await {
                    td.set(saved);
                }
            });
        }
    })
}

/// 初始化后若尚无 `/status` 快照则拉取一次。
pub fn wire_status_fetch_if_missing_after_init(
    initialized: RwSignal<bool>,
    st: StatusTasksSignals,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_status = Arc::clone(&refresh_status);
        move |_| {
            if initialized.get() && st.status_data.get().is_none() {
                refresh_status();
            }
        }
    });
}

/// 侧栏为「任务」且已初始化时拉取任务列表。
pub fn wire_tasks_refresh_when_tasks_panel_visible(
    side_panel_view: RwSignal<SidePanelView>,
    initialized: RwSignal<bool>,
    refresh_tasks: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_tasks = Arc::clone(&refresh_tasks);
        move |_| {
            if matches!(side_panel_view.get(), SidePanelView::Tasks) && initialized.get() {
                refresh_tasks();
            }
        }
    });
}

/// 初始化后补拉 `/status`；任务侧栏可见时刷新任务列表（从 `app/mod.rs` 迁入，阶段 B）。
pub fn wire_status_tasks_domain_effects(
    initialized: RwSignal<bool>,
    status_tasks: StatusTasksSignals,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
    side_panel_view: RwSignal<SidePanelView>,
    refresh_tasks: Arc<dyn Fn() + Send + Sync>,
) {
    wire_status_fetch_if_missing_after_init(initialized, status_tasks, Arc::clone(&refresh_status));
    wire_tasks_refresh_when_tasks_panel_visible(
        side_panel_view,
        initialized,
        Arc::clone(&refresh_tasks),
    );
}
