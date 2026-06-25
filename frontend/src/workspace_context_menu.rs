//! 工作区文件树右键菜单与行内新建（VS Code 风格：在树中直接输入名称）。

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::confirm_dialog::confirm_user_message;
use crate::i18n::Locale;
use crate::ide_tabs::{IdeTabsEditorSignals, IdeTabsHandle};

/// 工作区树右键菜单锚点（`position: fixed` 使用视口坐标）。
#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceContextAnchor {
    pub x: f64,
    pub y: f64,
    /// 右键目标相对路径；空白处为 `None`（表示工作区根）。
    pub target_rel: Option<String>,
    pub target_is_dir: bool,
    /// 新建项时的默认父目录（目录自身或其父路径；根为 `""`）。
    pub parent_rel: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceInlineCreateKind {
    File,
    Dir,
}

/// 行内新建：在 `parent_rel` 对应列表末尾显示输入框。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspacePendingCreate {
    pub parent_rel: String,
    pub kind: WorkspaceInlineCreateKind,
}

#[derive(Clone, Copy)]
pub struct WorkspaceTreeChromeSignals {
    pub context_menu: RwSignal<Option<WorkspaceContextAnchor>>,
    pub pending_create: RwSignal<Option<WorkspacePendingCreate>>,
}

/// 删除成功后传入，用于清理子树缓存/展开状态。
#[derive(Clone, Debug)]
pub struct WorkspaceTreeRefreshHint {
    pub parent_rel: String,
    pub deleted_rel: Option<String>,
}

#[derive(Clone)]
pub struct WorkspaceContextMenuActions {
    pub refresh_after_mutation: std::sync::Arc<dyn Fn(WorkspaceTreeRefreshHint) + Send + Sync>,
    pub ide_tabs: Option<(IdeTabsHandle, IdeTabsEditorSignals)>,
}

pub fn name_segment_valid(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && !n
            .chars()
            .any(|c| c.is_whitespace() || c == '/' || c == '\\')
        && n != "."
        && n != ".."
}

/// 右键菜单选择「新建」后：展开父目录（若有）并进入行内命名。
pub fn begin_workspace_inline_create(
    parent_rel: String,
    kind: WorkspaceInlineCreateKind,
    chrome: WorkspaceTreeChromeSignals,
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, crate::api::WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
) {
    chrome.context_menu.set(None);
    if !parent_rel.is_empty() {
        crate::workspace_tree::ensure_workspace_dir_open(
            parent_rel.clone(),
            subtree_expanded,
            subtree_cache,
            subtree_loading,
            locale,
        );
    }
    chrome
        .pending_create
        .set(Some(WorkspacePendingCreate { parent_rel, kind }));
}

fn delete_confirm_message(locale: Locale, path: &str, is_dir: bool) -> String {
    if is_dir {
        crate::i18n::workspace_tree_delete_dir_confirm(locale, path, true).to_string()
    } else {
        crate::i18n::workspace_tree_delete_file_confirm(locale, path).to_string()
    }
}

#[component]
pub fn WorkspaceContextMenuLayer(
    workspace_context_menu: RwSignal<Option<WorkspaceContextAnchor>>,
    workspace_pending_create: RwSignal<Option<WorkspacePendingCreate>>,
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, crate::api::WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
    workspace_err: RwSignal<Option<String>>,
    actions: WorkspaceContextMenuActions,
) -> impl IntoView {
    let chrome = WorkspaceTreeChromeSignals {
        context_menu: workspace_context_menu,
        pending_create: workspace_pending_create,
    };
    let actions = StoredValue::new(actions);
    view! {
        <Show when=move || workspace_context_menu.get().is_some()>
            <div class="session-ctx-layer workspace-ctx-layer">
                <div
                    class="session-ctx-backdrop"
                    aria-hidden="true"
                    on:click=move |_| workspace_context_menu.set(None)
                ></div>
                <div
                    class="session-ctx-menu workspace-ctx-menu"
                    role="menu"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    style=move || {
                        workspace_context_menu
                            .get()
                            .map(|a| format!("left:{}px;top:{}px;", a.x, a.y))
                            .unwrap_or_default()
                    }
                >
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(anchor) = workspace_context_menu.get() else {
                                return;
                            };
                            begin_workspace_inline_create(
                                anchor.parent_rel,
                                WorkspaceInlineCreateKind::File,
                                chrome,
                                subtree_expanded,
                                subtree_cache,
                                subtree_loading,
                                locale,
                            );
                        }
                    >
                        {move || crate::i18n::workspace_tree_ctx_new_file(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(anchor) = workspace_context_menu.get() else {
                                return;
                            };
                            begin_workspace_inline_create(
                                anchor.parent_rel,
                                WorkspaceInlineCreateKind::Dir,
                                chrome,
                                subtree_expanded,
                                subtree_cache,
                                subtree_loading,
                                locale,
                            );
                        }
                    >
                        {move || crate::i18n::workspace_tree_ctx_new_dir(locale.get())}
                    </button>
                    <Show when=move || {
                        workspace_context_menu
                            .get()
                            .and_then(|a| a.target_rel)
                            .is_some()
                    }>
                        <button
                            type="button"
                            class="session-ctx-item session-ctx-item-danger"
                            role="menuitem"
                            on:click=move |_| {
                                let Some(anchor) = workspace_context_menu.get() else {
                                    return;
                                };
                                let Some(rel) = anchor.target_rel.clone() else {
                                    return;
                                };
                                workspace_context_menu.set(None);
                                let loc = locale.get_untracked();
                                let msg = delete_confirm_message(loc, rel.as_str(), anchor.target_is_dir);
                                let is_dir = anchor.target_is_dir;
                                let actions = actions.get_value();
                                spawn_local(async move {
                                    if !confirm_user_message(&msg).await {
                                        return;
                                    }
                                    crate::workspace_fs_ops::spawn_delete_workspace_entry(
                                        rel,
                                        is_dir,
                                        locale,
                                        workspace_err,
                                        actions,
                                    );
                                });
                            }
                        >
                            {move || {
                                let loc = locale.get();
                                match workspace_context_menu.get() {
                                    Some(a) if a.target_is_dir => {
                                        crate::i18n::workspace_tree_ctx_delete_dir(loc).to_string()
                                    }
                                    Some(_) => {
                                        crate::i18n::workspace_tree_ctx_delete_file(loc).to_string()
                                    }
                                    None => String::new(),
                                }
                            }}
                        </button>
                    </Show>
                </div>
            </div>
        </Show>
    }
}
