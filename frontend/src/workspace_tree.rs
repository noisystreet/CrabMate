//! 工作区侧栏：可展开/折叠的子目录树（默认折叠，按需 `GET /workspace?path=` 拉取）。

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use gloo_timers::future::TimeoutFuture;
use leptos::html::Input;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;

use crate::api::{WorkspaceData, WorkspaceEntry, fetch_workspace};
use crate::i18n::{self, Locale};
use crate::workspace_context_menu::{
    WorkspaceContextMenuActions, WorkspaceInlineCreateKind, WorkspacePendingCreate,
    WorkspaceTreeChromeSignals,
};
use crate::workspace_fs_ops::commit_inline_create;
use crate::workspace_shell::{workspace_list_row_class, workspace_list_row_icon};

fn try_commit_inline_create_row(
    name: String,
    parent_rel: &str,
    kind: WorkspaceInlineCreateKind,
    chrome: WorkspaceTreeChromeSignals,
    locale: RwSignal<Locale>,
    workspace_err: RwSignal<Option<String>>,
    create_actions: WorkspaceContextMenuActions,
) {
    let name = name.trim().to_string();
    if name.is_empty() {
        chrome.pending_create.set(None);
        workspace_err.set(None);
        return;
    }
    if !crate::workspace_context_menu::name_segment_valid(&name) {
        workspace_err.set(Some(
            i18n::workspace_tree_name_invalid(locale.get_untracked()).to_string(),
        ));
        return;
    }
    chrome.pending_create.set(None);
    commit_inline_create(
        name,
        parent_rel,
        kind,
        locale,
        workspace_err,
        create_actions,
    );
}

/// 相对工作区根的路径片段拼接（POSIX 风格，与后端 `path` 查询一致）。
pub fn workspace_child_rel(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent, name)
    }
}

/// 条目相对路径的父目录（根下文件/夹返回 `""`）。
pub fn workspace_parent_rel(rel: &str) -> String {
    rel.rfind('/')
        .map(|i| rel[..i].to_string())
        .unwrap_or_default()
}

/// 将 `parent_rel` 的各级祖先（含自身）加入 `expanded`。
pub fn workspace_expand_ancestor_dirs(expanded: &mut HashSet<String>, parent_rel: &str) {
    if parent_rel.is_empty() {
        return;
    }
    let mut acc = String::new();
    for part in parent_rel.split('/') {
        if part.is_empty() {
            continue;
        }
        acc = workspace_child_rel(acc.as_str(), part);
        expanded.insert(acc.clone());
    }
}

/// 删除目录/文件后：移除 `deleted_rel` 及其子孙在展开集与缓存中的条目。
pub fn workspace_prune_subtree_state(
    expanded: &mut HashSet<String>,
    cache: &mut HashMap<String, WorkspaceData>,
    deleted_rel: &str,
) {
    if deleted_rel.is_empty() {
        return;
    }
    let prefix = format!("{deleted_rel}/");
    expanded.retain(|p| p != deleted_rel && !p.starts_with(prefix.as_str()));
    cache.retain(|p, _| p != deleted_rel && !p.starts_with(prefix.as_str()));
}

fn fetch_workspace_subtree_if_needed(
    rel: String,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
) {
    if subtree_cache.with(|m| m.contains_key(&rel)) {
        return;
    }
    if subtree_loading.with(|s| s.contains(&rel)) {
        return;
    }
    subtree_loading.update(|s| {
        s.insert(rel.clone());
    });
    spawn_local(async move {
        let path_key = rel.clone();
        let res = fetch_workspace(Some(&path_key), locale.get_untracked()).await;
        subtree_loading.update(|s| {
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

/// 展开目录并在尚无缓存时拉取子项（行内新建前调用）。
pub fn ensure_workspace_dir_open(
    rel: String,
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
) {
    subtree_expanded.update(|s| {
        s.insert(rel.clone());
    });
    fetch_workspace_subtree_if_needed(rel, subtree_cache, subtree_loading, locale);
}

fn toggle_workspace_dir(
    rel: String,
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
) {
    if subtree_expanded.with(|s| s.contains(&rel)) {
        subtree_expanded.update(|s| {
            s.remove(&rel);
        });
        return;
    }
    ensure_workspace_dir_open(
        rel,
        subtree_expanded,
        subtree_cache,
        subtree_loading,
        locale,
    );
}

fn workspace_filesystem_ul_class(entries_empty: bool, show_inline: bool) -> &'static str {
    if entries_empty && !show_inline {
        "workspace-list"
    } else {
        "workspace-list list-stagger"
    }
}

fn open_workspace_context_menu(
    ev: web_sys::MouseEvent,
    workspace_context_menu: RwSignal<Option<crate::workspace_context_menu::WorkspaceContextAnchor>>,
    target_rel: Option<String>,
    target_is_dir: bool,
    parent_rel: String,
) {
    ev.prevent_default();
    workspace_context_menu.set(Some(
        crate::workspace_context_menu::WorkspaceContextAnchor {
            x: ev.client_x() as f64,
            y: ev.client_y() as f64,
            target_rel,
            target_is_dir,
            parent_rel,
        },
    ));
}

fn context_menu_target_is_tree_row(ev: &web_sys::MouseEvent) -> bool {
    let Some(t) = ev.target() else {
        return false;
    };
    let Ok(el) = t.dyn_into::<web_sys::HtmlElement>() else {
        return false;
    };
    el.closest("li").ok().flatten().is_some()
}

fn context_menu_target_is_workspace_set(ev: &web_sys::MouseEvent) -> bool {
    let Some(t) = ev.target() else {
        return false;
    };
    let Ok(el) = t.dyn_into::<web_sys::HtmlElement>() else {
        return false;
    };
    el.closest(".workspace-set").ok().flatten().is_some()
}

/// 工作区树面板空白处（含列表下方留白）右键：在根目录新建。
pub(crate) fn handle_workspace_tree_panel_context_menu(
    ev: web_sys::MouseEvent,
    workspace_context_menu: RwSignal<Option<crate::workspace_context_menu::WorkspaceContextAnchor>>,
) {
    if context_menu_target_is_tree_row(&ev) || context_menu_target_is_workspace_set(&ev) {
        return;
    }
    open_workspace_context_menu(ev, workspace_context_menu, None, false, String::new());
}

#[derive(Clone, Copy)]
struct WorkspaceSubtreeSignals {
    subtree_expanded: RwSignal<HashSet<String>>,
    subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    subtree_loading: RwSignal<HashSet<String>>,
    locale: RwSignal<Locale>,
}

#[derive(Clone, Copy)]
struct WorkspaceTreeEnv {
    subtree: WorkspaceSubtreeSignals,
    chrome: WorkspaceTreeChromeSignals,
    create_actions: StoredValue<WorkspaceContextMenuActions>,
    on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    workspace_err: RwSignal<Option<String>>,
}

#[component]
fn WorkspaceTreeInlineCreateRow(
    parent_rel: String,
    kind: WorkspaceInlineCreateKind,
    env: WorkspaceTreeEnv,
) -> impl IntoView {
    let WorkspaceTreeEnv {
        chrome,
        create_actions,
        workspace_err,
        subtree,
        ..
    } = env;
    let locale = subtree.locale;
    let draft = RwSignal::new(String::new());
    let input_ref = NodeRef::<Input>::new();
    let is_dir = kind == WorkspaceInlineCreateKind::Dir;
    let row_class = workspace_list_row_class(is_dir, "");
    let parent_for_commit = StoredValue::new(parent_rel);

    Effect::new(move |_| {
        let _ = chrome.pending_create.get();
        let node = input_ref.get();
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if let Some(el) = node {
                let _ = el.focus();
                el.select();
            }
        });
    });

    let cancel = move || {
        chrome.pending_create.set(None);
        workspace_err.set(None);
    };

    view! {
        <li class=format!("{row_class} workspace-tree-inline-create") role="treeitem">
            {workspace_list_row_icon(is_dir, "")}
            <input
                node_ref=input_ref
                type="text"
                class="workspace-tree-inline-input"
                prop:placeholder=move || {
                    match kind {
                        WorkspaceInlineCreateKind::File => {
                            i18n::workspace_tree_inline_name_ph_file(locale.get())
                        }
                        WorkspaceInlineCreateKind::Dir => {
                            i18n::workspace_tree_inline_name_ph_dir(locale.get())
                        }
                    }
                }
                prop:value=move || draft.get()
                on:input=move |ev| {
                    draft.set(event_target_value(&ev));
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        ev.prevent_default();
                        ev.stop_propagation();
                        cancel();
                        return;
                    }
                    if ev.key() == "Enter" {
                        ev.prevent_default();
                        ev.stop_propagation();
                        try_commit_inline_create_row(
                            draft.get_untracked(),
                            parent_for_commit.get_value().as_str(),
                            kind,
                            chrome,
                            locale,
                            workspace_err,
                            create_actions.get_value(),
                        );
                    }
                }
                on:blur=move |_| {
                    let draft = draft;
                    let parent = parent_for_commit;
                    let chrome = chrome;
                    let create_actions = create_actions;
                    spawn_local(async move {
                        // 略延迟，避免与其它 click 竞态；空白处点击失焦时与 Enter 同样提交。
                        TimeoutFuture::new(120).await;
                        if chrome.pending_create.get_untracked().is_none() {
                            return;
                        }
                        try_commit_inline_create_row(
                            draft.get_untracked(),
                            parent.get_value().as_str(),
                            kind,
                            chrome,
                            locale,
                            workspace_err,
                            create_actions.get_value(),
                        );
                    });
                }
            />
        </li>
    }
}

fn pending_create_at_parent(
    chrome: WorkspaceTreeChromeSignals,
    parent_rel: &str,
) -> Option<WorkspacePendingCreate> {
    chrome
        .pending_create
        .get()
        .filter(|p| p.parent_rel == parent_rel)
}

#[component]
fn WorkspaceTreeNodes(
    parent_rel: String,
    entries: Vec<WorkspaceEntry>,
    base_stagger: u32,
    env: WorkspaceTreeEnv,
) -> impl IntoView {
    let parent_for_inline = StoredValue::new(parent_rel);
    view! {
        <>
            {entries
                .into_iter()
                .enumerate()
                .map(|(i, e)| {
                    let stagger = (base_stagger as usize + i).to_string();
                    let name = e.name.clone();
                    let is_dir = e.is_dir;
                    let row_class = workspace_list_row_class(is_dir, name.as_str());
                    let rel = workspace_child_rel(parent_for_inline.get_value().as_str(), &name);
                    let parent_for_row = parent_for_inline.get_value();
                    if !is_dir {
                        view! {
                            <WorkspaceTreeFileRow
                                row_class=row_class
                                stagger=stagger
                                name=name
                                rel=rel
                                parent_rel=parent_for_row
                                chrome=env.chrome
                                on_file_double_click=env.on_file_double_click
                                on_file_single_click=env.on_file_single_click
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
                                env=env
                            />
                        }
                        .into_any()
                    }
                })
                .collect_view()}
            <Show when=move || {
                pending_create_at_parent(
                    env.chrome,
                    parent_for_inline.get_value().as_str(),
                )
                .is_some()
            }>
                {move || {
                    let pending = pending_create_at_parent(
                        env.chrome,
                        parent_for_inline.get_value().as_str(),
                    )?;
                    Some(view! {
                        <WorkspaceTreeInlineCreateRow
                            parent_rel=parent_for_inline.get_value()
                            kind=pending.kind
                            env=env
                        />
                    })
                }}
            </Show>
        </>
    }
}

#[component]
fn WorkspaceTreeFileRow(
    row_class: String,
    stagger: String,
    name: String,
    rel: String,
    parent_rel: String,
    chrome: WorkspaceTreeChromeSignals,
    on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    let rel_dbl = rel.clone();
    let rel_click = rel.clone();
    let rel_ctx = rel.clone();
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
            on:contextmenu=move |ev: web_sys::MouseEvent| {
                open_workspace_context_menu(
                    ev,
                    chrome.context_menu,
                    Some(rel_ctx.clone()),
                    false,
                    parent_rel.clone(),
                );
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
    env: WorkspaceTreeEnv,
) -> impl IntoView {
    let WorkspaceTreeEnv {
        subtree:
            WorkspaceSubtreeSignals {
                subtree_expanded,
                subtree_cache,
                subtree_loading,
                locale,
            },
        chrome,
        ..
    } = env;
    let rel_aria = rel.clone();
    let rel_aria_label = rel.clone();
    let rel_click = rel.clone();
    let rel_glyph = rel.clone();
    let rel_show = rel.clone();
    let rel_inner = StoredValue::new(rel);
    let rel_ctx = rel_aria_label.clone();
    let name_for_aria = name.clone();
    let env_nested = env;
    view! {
        <li
            class=format!("{row_class} workspace-dir-node")
            style=format!("--list-stagger: {stagger}")
        >
            <div
                class="workspace-dir-head"
                on:contextmenu=move |ev: web_sys::MouseEvent| {
                    open_workspace_context_menu(
                        ev,
                        chrome.context_menu,
                        Some(rel_ctx.clone()),
                        true,
                        rel_ctx.clone(),
                    );
                }
            >
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
                    let pending_here = env_nested
                        .chrome
                        .pending_create
                        .get()
                        .is_some_and(|pc| pc.parent_rel == p);
                    if loading && cached.is_none() && !pending_here {
                        view! {
                            <p class="workspace-tree-loading" role="status">
                                {move || i18n::changelist_loading(locale.get())}
                            </p>
                        }
                        .into_any()
                    } else if cached.as_ref().and_then(|d| d.error.as_ref()).is_some() {
                        let err = cached
                            .as_ref()
                            .and_then(|d| d.error.clone())
                            .unwrap_or_default();
                        view! {
                            <ul class="workspace-list workspace-list-nested" role="group">
                                <li class="msg-error workspace-tree-err">{err}</li>
                                <WorkspaceTreeNodes
                                    parent_rel=p.clone()
                                    entries=Vec::new()
                                    base_stagger=0
                                    env=env_nested
                                />
                            </ul>
                        }
                        .into_any()
                    } else {
                        let nested = cached
                            .as_ref()
                            .map(|d| d.entries.clone())
                            .unwrap_or_default();
                        view! {
                            <ul
                                class="workspace-list workspace-list-nested"
                                role="group"
                            >
                                <WorkspaceTreeNodes
                                    parent_rel=p.clone()
                                    entries=nested
                                    base_stagger=0
                                    env=env_nested
                                />
                            </ul>
                        }
                        .into_any()
                    }
                }}
            </Show>
        </li>
    }
}

/// 根级工作区文件树组件入参（控制形参个数棘轮）。
#[derive(Clone, Copy)]
pub struct WorkspaceFilesystemTreeInput {
    pub workspace_data: RwSignal<Option<WorkspaceData>>,
    pub subtree_expanded: RwSignal<HashSet<String>>,
    pub subtree_cache: RwSignal<HashMap<String, WorkspaceData>>,
    pub subtree_loading: RwSignal<HashSet<String>>,
    pub chrome: WorkspaceTreeChromeSignals,
    pub locale: RwSignal<Locale>,
    pub workspace_err: RwSignal<Option<String>>,
    pub create_actions: StoredValue<WorkspaceContextMenuActions>,
    pub on_file_double_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    pub on_file_single_click: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
}

/// 根级 `ul` 内：空数据占位或树节点列表。
#[component]
pub fn WorkspaceFilesystemTree(input: WorkspaceFilesystemTreeInput) -> impl IntoView {
    let WorkspaceFilesystemTreeInput {
        workspace_data,
        subtree_expanded,
        subtree_cache,
        subtree_loading,
        chrome,
        locale,
        workspace_err,
        create_actions,
        on_file_double_click,
        on_file_single_click,
    } = input;
    let env = WorkspaceTreeEnv {
        subtree: WorkspaceSubtreeSignals {
            subtree_expanded,
            subtree_cache,
            subtree_loading,
            locale,
        },
        chrome,
        create_actions,
        on_file_double_click,
        on_file_single_click,
        workspace_err,
    };
    view! {
        <ul
            data-testid="workspace-file-tree"
            prop:title=move || i18n::workspace_tree_insert_file_title(locale.get())
            class=move || {
                let entries = workspace_data
                    .get()
                    .map(|d| d.entries)
                    .unwrap_or_default();
                let pending_root = env
                    .chrome
                    .pending_create
                    .get()
                    .is_some_and(|p| p.parent_rel.is_empty());
                workspace_filesystem_ul_class(entries.is_empty(), pending_root)
            }
        >
            {move || {
                let entries = workspace_data
                    .get()
                    .map(|d| d.entries)
                    .unwrap_or_default();
                let pending_root = env
                    .chrome
                    .pending_create
                    .get()
                    .is_some_and(|p| p.parent_rel.is_empty());
                if entries.is_empty() && !pending_root {
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
                            env=env
                        />
                    }
                    .into_any()
                }
            }}
        </ul>
    }
}
