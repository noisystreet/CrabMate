//! 工作区文件树：磁盘新建/删除异步操作。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{
    delete_workspace_dir, delete_workspace_file, post_workspace_dir, post_workspace_file_write_opts,
};
use crate::i18n::Locale;
use crate::ide_save::spawn_create_and_open_file;
use crate::ide_tabs::force_close_tabs_for_deleted_entry;
use crate::workspace_context_menu::{
    WorkspaceContextMenuActions, WorkspaceInlineCreateKind, WorkspaceTreeRefreshHint,
};
use crate::workspace_tree::workspace_parent_rel;

fn refresh_after_create(actions: &WorkspaceContextMenuActions, parent_rel: String) {
    (actions.refresh_after_mutation)(WorkspaceTreeRefreshHint {
        parent_rel,
        deleted_rel: None,
    });
}

fn refresh_after_delete(actions: &WorkspaceContextMenuActions, deleted_rel: String) {
    let parent_rel = workspace_parent_rel(deleted_rel.as_str());
    (actions.refresh_after_mutation)(WorkspaceTreeRefreshHint {
        parent_rel,
        deleted_rel: Some(deleted_rel),
    });
}

pub fn spawn_create_workspace_file(
    rel: String,
    locale: RwSignal<Locale>,
    workspace_err: RwSignal<Option<String>>,
    actions: WorkspaceContextMenuActions,
) {
    let parent_rel = workspace_parent_rel(rel.as_str());
    spawn_local(async move {
        let loc = locale.get_untracked();
        match post_workspace_file_write_opts(rel, String::new(), true, false, loc).await {
            Ok(()) => {
                workspace_err.set(None);
                refresh_after_create(&actions, parent_rel);
            }
            Err(e) => workspace_err.set(Some(e)),
        }
    });
}

pub fn spawn_create_workspace_dir(
    rel: String,
    locale: RwSignal<Locale>,
    workspace_err: RwSignal<Option<String>>,
    actions: WorkspaceContextMenuActions,
) {
    let parent_rel = workspace_parent_rel(rel.as_str());
    spawn_local(async move {
        let loc = locale.get_untracked();
        match post_workspace_dir(rel, true, loc).await {
            Ok(()) => {
                workspace_err.set(None);
                refresh_after_create(&actions, parent_rel);
            }
            Err(e) => workspace_err.set(Some(e)),
        }
    });
}

pub fn spawn_delete_workspace_entry(
    rel: String,
    is_dir: bool,
    locale: RwSignal<Locale>,
    workspace_err: RwSignal<Option<String>>,
    actions: WorkspaceContextMenuActions,
) {
    spawn_local(async move {
        let loc = locale.get_untracked();
        let deleted = rel.clone();
        let result = if is_dir {
            delete_workspace_dir(rel.as_str(), true, loc).await
        } else {
            delete_workspace_file(rel.as_str(), loc).await
        };
        match &result {
            Ok(()) => {
                workspace_err.set(None);
                if let Some((tabs, editor)) = actions.ide_tabs {
                    force_close_tabs_for_deleted_entry(tabs, deleted.as_str(), is_dir, editor);
                }
                refresh_after_delete(&actions, deleted);
            }
            Err(e) => workspace_err.set(Some(e.clone())),
        }
    });
}

/// 行内输入确认：校验名称并创建文件或目录。
pub fn commit_inline_create(
    name: String,
    parent_rel: &str,
    kind: WorkspaceInlineCreateKind,
    locale: RwSignal<Locale>,
    workspace_err: RwSignal<Option<String>>,
    actions: WorkspaceContextMenuActions,
) {
    let name = name.trim().to_string();
    if !crate::workspace_context_menu::name_segment_valid(&name) {
        workspace_err.set(Some(
            crate::i18n::workspace_tree_name_invalid(locale.get_untracked()).to_string(),
        ));
        return;
    }
    let rel = crate::workspace_tree::workspace_child_rel(parent_rel, &name);
    let parent_for_refresh = parent_rel.to_string();
    workspace_err.set(None);
    match kind {
        WorkspaceInlineCreateKind::File => {
            if let Some((tabs, editor)) = actions.ide_tabs {
                let after_create = {
                    let refresh = Arc::clone(&actions.refresh_after_mutation);
                    let parent = parent_for_refresh.clone();
                    Arc::new(move || {
                        refresh(WorkspaceTreeRefreshHint {
                            parent_rel: parent.clone(),
                            deleted_rel: None,
                        })
                    })
                };
                spawn_create_and_open_file(
                    crate::ide_save::IdeSaveContext {
                        tabs,
                        ide_path: editor.ide_path,
                        ide_text: editor.ide_text,
                        ide_baseline: editor.ide_baseline,
                        ide_err: tabs.err,
                    },
                    locale,
                    rel,
                    Some(after_create),
                );
            } else {
                spawn_create_workspace_file(rel, locale, workspace_err, actions);
            }
        }
        WorkspaceInlineCreateKind::Dir => {
            spawn_create_workspace_dir(rel, locale, workspace_err, actions);
        }
    }
}
