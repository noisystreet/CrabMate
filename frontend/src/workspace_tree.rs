//! 工作区侧栏：可展开/折叠的子目录树（默认折叠，按需 `GET /workspace?path=` 拉取）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{WorkspaceData, WorkspaceEntry, fetch_workspace};
use crate::i18n::{self, Locale};
use crate::workspace_shell::{workspace_list_row_class, workspace_list_row_icon};

/// 相对工作区根的路径片段拼接（POSIX 风格，与后端 `path` 查询一致）。
pub fn workspace_child_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent, name)
    }
}

fn toggle_workspace_dir(
    rel: String,
    expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    loading_paths: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
) {
    if expanded.with(|s| s.contains(&rel)) {
        expanded.update(|s| {
            s.remove(&rel);
        });
        return;
    }
    expanded.update(|s| {
        s.insert(rel.clone());
    });
    if subtree_cache.with(|m| m.contains_key(&rel)) {
        return;
    }
    loading_paths.update(|s| {
        s.insert(rel.clone());
    });
    let loc = locale.get();
    spawn_local(async move {
        let path_key = rel.clone();
        let res = fetch_workspace(Some(&path_key), loc).await;
        loading_paths.update(|s| {
            s.remove(&path_key);
        });
        match res {
            Ok(d) => {
                subtree_cache.update(|m| {
                    m.insert(path_key, d);
                });
            }
            Err(e) => {
                subtree_cache.update(|m| {
                    m.insert(
                        path_key,
                        WorkspaceData {
                            path: String::new(),
                            entries: Vec::new(),
                            error: Some(e),
                        },
                    );
                });
            }
        }
    });
}

fn workspace_filesystem_ul_class(entries_empty: bool) -> &'static str {
    if entries_empty {
        "workspace-list"
    } else {
        "workspace-list list-stagger"
    }
}

#[derive(Clone, Copy)]
struct WorkspaceSubtreeSignals {
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
}

#[component]
fn WorkspaceTreeFileRow(
    row_class: String,
    stagger: String,
    name: String,
    rel: String,
    on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    let rel_dbl = rel.clone();
    let rel_click = rel.clone();
    view! {
        <li
            class=row_class
            style=format!("--list-stagger: {stagger}")
            on:click=move |_| {
                (on_file_single_click.get_value())(rel_click.clone());
            }
            on:dblclick=move |_| {
                (on_file_double_click.get_value())(rel_dbl.clone());
            }
        >
            {workspace_list_row_icon(false, name.as_str())}
            <span class="workspace-entry-name">{name}</span>
        </li>
    }
}

#[component]
fn WorkspaceTreeDirectoryNode(
    row_class: String,
    stagger: String,
    name: String,
    rel: String,
    subtree: WorkspaceSubtreeSignals,
    on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    let WorkspaceSubtreeSignals {
        subtree_expanded,
        subtree_cache,
        subtree_loading,
        locale,
    } = subtree;
    let rel_aria = rel.clone();
    let rel_aria_label = rel.clone();
    let rel_click = rel.clone();
    let rel_glyph = rel.clone();
    let rel_show = rel.clone();
    let rel_inner = StoredValue::new(rel);
    let name_for_aria = name.clone();
    view! {
        <li
            class=format!("{row_class} workspace-dir-node")
            style=format!("--list-stagger: {stagger}")
        >
            <div class="workspace-dir-head">
                <button
                    type="button"
                    class="workspace-tree-chevron"
                    aria-expanded=move || subtree_expanded.get().contains(&rel_aria)
                    prop:aria-label=move || {
                        let loc = locale.get();
                        if subtree_expanded.get().contains(&rel_aria_label) {
                            i18n::workspace_tree_collapse_folder(loc, name_for_aria.as_str())
                        } else {
                            i18n::workspace_tree_expand_folder(loc, name_for_aria.as_str())
                        }
                    }
                    prop:title=move || i18n::workspace_tree_toggle_dir_title(locale.get())
                    on:click=move |_| {
                        toggle_workspace_dir(
                            rel_click.clone(),
                            subtree_expanded,
                            subtree_cache,
                            subtree_loading,
                            locale,
                        );
                    }
                >
                    {move || {
                        if subtree_expanded.get().contains(&rel_glyph) {
                            "▾"
                        } else {
                            "▸"
                        }
                    }}
                </button>
                {workspace_list_row_icon(true, name.as_str())}
                <span class="workspace-entry-name">{name}</span>
            </div>
            <Show when=move || subtree_expanded.get().contains(&rel_show)>
                {move || {
                    let p = rel_inner.get_value();
                    let loading = subtree_loading.get().contains(&p);
                    let cached = subtree_cache.get().get(&p).cloned();
                    if loading && cached.is_none() {
                        view! {
                            <p class="workspace-tree-loading" role="status">
                                {move || i18n::changelist_loading(locale.get())}
                            </p>
                        }
                        .into_any()
                    } else if let Some(d) = cached {
                        if let Some(err) = d.error.clone() {
                            view! {
                                <p class="msg-error workspace-tree-err">{err}</p>
                            }
                            .into_any()
                        } else {
                            let nested = d.entries.clone();
                            let rel_for_nested = p.clone();
                            view! {
                                <ul
                                    class="workspace-list workspace-list-nested"
                                    role="group"
                                >
                                    <WorkspaceTreeNodes
                                        parent_rel=rel_for_nested
                                        entries=nested
                                        base_stagger=0
                                        subtree=subtree
                                        on_file_double_click=on_file_double_click
                                        on_file_single_click=on_file_single_click
                                    />
                                </ul>
                            }
                            .into_any()
                        }
                    } else {
                        view! { <p class="workspace-tree-placeholder">" "</p> }.into_any()
                    }
                }}
            </Show>
        </li>
    }
}

/// 根级 `ul` 内：空数据占位或树节点列表。
#[component]
pub fn WorkspaceFilesystemTree(
    workspace_data: RwSignal<Option<WorkspaceData>>,
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
    /// 双击工作区树中的**文件**行时回调相对路径（POSIX）；目录行不触发。
    on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    /// 单击文件行时回调（IDE 布局打开编辑器；侧栏可传 no-op）。
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    view! {
        <ul
            data-testid="workspace-file-tree"
            prop:title=move || i18n::workspace_tree_insert_file_title(locale.get())
            class=move || {
                let entries = workspace_data
                    .get()
                    .map(|d| d.entries)
                    .unwrap_or_default();
                workspace_filesystem_ul_class(entries.is_empty())
            }
        >
            {move || {
                let entries = workspace_data
                    .get()
                    .map(|d| d.entries)
                    .unwrap_or_default();
                if entries.is_empty() {
                    view! {
                        <li>{move || i18n::workspace_tree_no_data(locale.get())}</li>
                    }
                    .into_any()
                } else {
                    view! {
                        <WorkspaceTreeNodes
                            parent_rel=String::new()
                            entries=entries
                            base_stagger=0_u32
                            subtree=WorkspaceSubtreeSignals {
                                subtree_expanded,
                                subtree_cache,
                                subtree_loading,
                                locale,
                            }
                            on_file_double_click=on_file_double_click
                            on_file_single_click=on_file_single_click
                        />
                    }
                    .into_any()
                }
            }}
        </ul>
    }
}

#[component]
fn WorkspaceTreeNodes(
    parent_rel: String,
    entries: Vec<WorkspaceEntry>,
    /// 根列表用于 `list-stagger` 的全局序号起点。
    base_stagger: u32,
    subtree: WorkspaceSubtreeSignals,
    on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    entries
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let stagger = (base_stagger as usize + i).to_string();
            let name = e.name.clone();
            let is_dir = e.is_dir;
            let row_class = workspace_list_row_class(is_dir, name.as_str());
            let rel = workspace_child_rel(&parent_rel, &name);
            if !is_dir {
                view! {
                    <WorkspaceTreeFileRow
                        row_class=row_class
                        stagger=stagger
                        name=name
                        rel=rel
                        on_file_double_click=on_file_double_click
                        on_file_single_click=on_file_single_click
                    />
                }
                .into_any()
            } else {
                view! {
                    <WorkspaceTreeDirectoryNode
                        row_class=row_class
                        stagger=stagger
                        name=name
                        rel=rel
                        subtree=subtree
                        on_file_double_click=on_file_double_click
                        on_file_single_click=on_file_single_click
                    />
                }
                .into_any()
            }
        })
        .collect_view()
}
