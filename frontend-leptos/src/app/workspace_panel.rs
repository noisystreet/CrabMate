//! Workspace 侧栏：拉取目录树与在切换到 Workspace 视图时自动刷新；以及树双击插入 **`@路径`** 到输入框。

use std::sync::{Arc, Mutex};

use gloo_timers::future::TimeoutFuture;
use leptos::html::Textarea;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::app_prefs::SidePanelView;
use crate::i18n::Locale;
use crate::workspace_shell::reload_workspace_panel;

use super::workspace_panel_state::WorkspacePanelSignals;

/// 返回与历史行为一致的 `reload_workspace_panel` 封装（供 SSE `on_workspace_changed`、侧栏等复用）。
pub(super) fn make_refresh_workspace(ws: WorkspacePanelSignals) -> Arc<dyn Fn() + Send + Sync> {
    Arc::new(move || {
        spawn_local(async move {
            reload_workspace_panel(
                ws.workspace_loading,
                ws.workspace_err,
                ws.workspace_path_draft,
                ws.workspace_data,
                ws.workspace_subtree_expanded,
                ws.workspace_subtree_cache,
                ws.workspace_subtree_loading,
            )
            .await;
        });
    })
}

/// 工作区树双击文件时，将 **`@{rel}`** 插入 composer 草稿并聚焦输入框。
pub(super) fn make_insert_workspace_path_into_composer(
    composer_draft_buffer: Arc<Mutex<String>>,
    draft: RwSignal<String>,
    status_err: RwSignal<Option<String>>,
    locale: RwSignal<Locale>,
    composer_input_ref: NodeRef<Textarea>,
) -> Arc<dyn Fn(String) + Send + Sync> {
    Arc::new(move |rel: String| {
        if rel.chars().any(|c| c.is_whitespace()) {
            status_err.set(Some(
                crate::i18n::composer_ws_path_whitespace_err(locale.get_untracked()).to_string(),
            ));
            return;
        }
        let token = format!("@{rel}");
        let mut guard = composer_draft_buffer.lock().unwrap();
        let needs_space = guard
            .chars()
            .next_back()
            .is_some_and(|c| !c.is_whitespace());
        if needs_space {
            guard.push(' ');
        }
        guard.push_str(&token);
        guard.push(' ');
        let next = guard.clone();
        drop(guard);
        draft.set(next.clone());
        status_err.set(None);
        let cref = composer_input_ref.clone();
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if let Some(el) = cref.get() {
                let _ = el.focus();
            }
        });
    })
}

/// 初始化完成且侧栏为 Workspace 时拉取一次。
pub(super) fn wire_workspace_refresh_when_visible(
    side_panel_view: RwSignal<SidePanelView>,
    initialized: RwSignal<bool>,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let refresh_workspace = Arc::clone(&refresh_workspace);
        move |_| {
            if matches!(side_panel_view.get(), SidePanelView::Workspace) && initialized.get() {
                refresh_workspace();
            }
        }
    });
}
