//! IDE 多标签页：打开、切换、关闭与工作区文件加载。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{WorkspaceFileReadData, fetch_workspace_file};
use crate::i18n::{self, Locale};
use crate::ide_confirm::{IdeConfirmSignals, ide_confirm_user};

/// 单个已打开文件的缓冲。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeTab {
    pub path: String,
    pub text: String,
    pub baseline: String,
    pub pinned: bool,
}

/// 活动标签与编辑器缓冲区的共享信号。
#[derive(Clone, Copy)]
pub struct IdeTabsEditorSignals {
    pub ide_path: RwSignal<Option<String>>,
    pub ide_text: RwSignal<String>,
    pub ide_baseline: RwSignal<String>,
}

/// IDE 标签集合与活动标签索引（`active == None` 表示无打开文件）。
#[derive(Clone, Copy)]
pub struct IdeTabsHandle {
    pub tabs: RwSignal<Vec<IdeTab>>,
    pub active: RwSignal<Option<usize>>,
    pub load_busy: RwSignal<bool>,
    pub save_busy: RwSignal<bool>,
    pub err: RwSignal<Option<String>>,
}

impl IdeTabsHandle {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tabs: RwSignal::new(Vec::new()),
            active: RwSignal::new(None),
            load_busy: RwSignal::new(false),
            save_busy: RwSignal::new(false),
            err: RwSignal::new(None),
        }
    }

    pub fn persist_editor_into_active(
        &self,
        ide_text: RwSignal<String>,
        ide_baseline: RwSignal<String>,
    ) {
        let Some(idx) = self.active.get_untracked() else {
            return;
        };
        let text = ide_text.get_untracked();
        let baseline = ide_baseline.get_untracked();
        self.tabs.update(|tabs| {
            if let Some(tab) = tabs.get_mut(idx) {
                tab.text = text;
                tab.baseline = baseline;
            }
        });
    }

    pub fn load_active_into_editor(
        &self,
        ide_path: RwSignal<Option<String>>,
        ide_text: RwSignal<String>,
        ide_baseline: RwSignal<String>,
    ) {
        let Some(idx) = self.active.get_untracked() else {
            ide_path.set(None);
            ide_text.set(String::new());
            ide_baseline.set(String::new());
            return;
        };
        if let Some(tab) = self.tabs.get_untracked().get(idx) {
            ide_path.set(Some(tab.path.clone()));
            ide_text.set(tab.text.clone());
            ide_baseline.set(tab.baseline.clone());
        }
    }

    pub fn switch_to(
        &self,
        index: usize,
        ide_path: RwSignal<Option<String>>,
        ide_text: RwSignal<String>,
        ide_baseline: RwSignal<String>,
    ) {
        self.persist_editor_into_active(ide_text, ide_baseline);
        self.active.set(Some(index));
        self.load_active_into_editor(ide_path, ide_text, ide_baseline);
        self.err.set(None);
    }

    fn index_of_path(&self, path: &str) -> Option<usize> {
        self.tabs
            .get_untracked()
            .iter()
            .position(|t| t.path == path)
    }

    pub fn active_editor_is_dirty(
        &self,
        ide_text: RwSignal<String>,
        ide_baseline: RwSignal<String>,
    ) -> bool {
        self.active.get_untracked().is_some()
            && ide_text.get_untracked() != ide_baseline.get_untracked()
    }

    pub fn tab_display_dirty(
        &self,
        index: usize,
        ide_text: RwSignal<String>,
        ide_baseline: RwSignal<String>,
    ) -> bool {
        if self.active.get_untracked() == Some(index) {
            return self.active_editor_is_dirty(ide_text, ide_baseline);
        }
        self.tabs
            .get_untracked()
            .get(index)
            .is_some_and(|t| t.text != t.baseline)
    }
}

#[must_use]
pub fn ide_tab_basename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

async fn confirm_discard(locale: Locale, confirm: IdeConfirmSignals) -> bool {
    ide_confirm_user(confirm, i18n::ide_dirty_confirm(locale).to_string()).await
}

fn tab_is_dirty(
    tabs: IdeTabsHandle,
    index: usize,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
) -> bool {
    tabs.tab_display_dirty(index, ide_text, ide_baseline)
}

fn reorder_tabs_pinned_first(list: &mut Vec<IdeTab>) -> Vec<(usize, usize)> {
    let mut pinned = Vec::new();
    let mut unpinned = Vec::new();
    for (i, tab) in list.iter().enumerate() {
        if tab.pinned {
            pinned.push(i);
        } else {
            unpinned.push(i);
        }
    }
    let order: Vec<usize> = pinned.into_iter().chain(unpinned).collect();
    let old = std::mem::take(list);
    for &i in &order {
        list.push(old[i].clone());
    }
    order
        .into_iter()
        .enumerate()
        .map(|(new_i, old_i)| (old_i, new_i))
        .collect()
}

fn remap_active_after_reorder(
    remap: &[(usize, usize)],
    prev_active: Option<usize>,
) -> Option<usize> {
    prev_active.and_then(|old| remap.iter().find(|(o, _)| *o == old).map(|(_, n)| *n))
}

pub fn toggle_tab_pinned(
    tabs: IdeTabsHandle,
    index: usize,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
) {
    if index >= tabs.tabs.get_untracked().len() {
        return;
    }
    tabs.persist_editor_into_active(ide_text, ide_baseline);
    let prev_active = tabs.active.get_untracked();
    let mut remap = Vec::new();
    tabs.tabs.update(|list| {
        if let Some(tab) = list.get_mut(index) {
            tab.pinned = !tab.pinned;
        }
        remap = reorder_tabs_pinned_first(list);
    });
    tabs.active
        .set(remap_active_after_reorder(&remap, prev_active));
    tabs.load_active_into_editor(ide_path, ide_text, ide_baseline);
}

async fn confirm_discard_if_any_dirty(
    tabs: IdeTabsHandle,
    indices: &[usize],
    locale: Locale,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) -> bool {
    let IdeTabsEditorSignals {
        ide_text,
        ide_baseline,
        ..
    } = editor;
    let any_dirty = indices
        .iter()
        .any(|&i| tab_is_dirty(tabs, i, ide_text, ide_baseline));
    if !any_dirty {
        return true;
    }
    confirm_discard(locale, confirm).await
}

/// 批量关闭标签；`should_keep` 为 `true` 的条目保留。
pub async fn close_tabs_where(
    tabs: IdeTabsHandle,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
    should_keep: impl Fn(usize, &IdeTab) -> bool,
) {
    let IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    } = editor;
    tabs.persist_editor_into_active(ide_text, ide_baseline);
    let old_list = tabs.tabs.get_untracked();
    let to_close: Vec<usize> = old_list
        .iter()
        .enumerate()
        .filter(|(i, t)| !should_keep(*i, t))
        .map(|(i, _)| i)
        .collect();
    if to_close.is_empty() {
        return;
    }
    if !confirm_discard_if_any_dirty(tabs, &to_close, locale.get_untracked(), editor, confirm).await
    {
        return;
    }

    let prev_active = tabs.active.get_untracked();
    let mut new_list = Vec::with_capacity(old_list.len().saturating_sub(to_close.len()));
    let mut new_active = None;
    for (i, tab) in old_list.into_iter().enumerate() {
        if should_keep(i, &tab) {
            if prev_active == Some(i) {
                new_active = Some(new_list.len());
            }
            new_list.push(tab);
        }
    }
    tabs.tabs.set(new_list);
    tabs.active.set(new_active);
    tabs.load_active_into_editor(ide_path, ide_text, ide_baseline);
    if new_active.is_none() {
        tabs.err.set(None);
    }
}

pub async fn close_other_tabs_at(
    tabs: IdeTabsHandle,
    index: usize,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) {
    close_tabs_where(tabs, locale, editor, confirm, |i, tab| {
        i == index || tab.pinned
    })
    .await;
}

pub async fn close_all_tabs(
    tabs: IdeTabsHandle,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) {
    close_tabs_where(tabs, locale, editor, confirm, |_, _| false).await;
}

pub fn apply_fetch_to_new_tab(
    tabs: IdeTabsHandle,
    rel: String,
    content: String,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
) {
    tabs.tabs.update(|list| {
        list.push(IdeTab {
            path: rel.clone(),
            text: content.clone(),
            baseline: content.clone(),
            pinned: false,
        });
    });
    let idx = tabs.tabs.get_untracked().len().saturating_sub(1);
    tabs.active.set(Some(idx));
    ide_path.set(Some(rel));
    ide_text.set(content.clone());
    ide_baseline.set(content);
    tabs.err.set(None);
}

fn apply_fetch_error(tabs: IdeTabsHandle, err: String, ide_path: RwSignal<Option<String>>) {
    tabs.err.set(Some(err));
    ide_path.set(None);
}

pub fn wire_ide_editor_sync_to_active_tab(
    tabs: IdeTabsHandle,
    active: RwSignal<Option<usize>>,
    ide_text: RwSignal<String>,
) {
    Effect::new(move |_| {
        let _ = active.get();
        let text = ide_text.get();
        if let Some(i) = active.get_untracked() {
            tabs.tabs.update(|list| {
                if let Some(tab) = list.get_mut(i) {
                    tab.text = text;
                }
            });
        }
    });
}

pub async fn try_switch_tab(
    tabs: IdeTabsHandle,
    index: usize,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) -> bool {
    let IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    } = editor;
    if tabs.active.get_untracked() == Some(index) {
        return true;
    }
    if tabs.active_editor_is_dirty(ide_text, ide_baseline)
        && !confirm_discard(locale.get_untracked(), confirm).await
    {
        return false;
    }
    tabs.switch_to(index, ide_path, ide_text, ide_baseline);
    true
}

pub async fn close_tab_at(
    tabs: IdeTabsHandle,
    index: usize,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) {
    let IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    } = editor;
    let closing_active = tabs.active.get_untracked() == Some(index);
    if closing_active && tabs.active_editor_is_dirty(ide_text, ide_baseline) {
        if !confirm_discard(locale.get_untracked(), confirm).await {
            return;
        }
    } else if !closing_active {
        if let Some(tab) = tabs.tabs.get_untracked().get(index)
            && tab.text != tab.baseline
            && !confirm_discard(locale.get_untracked(), confirm).await
        {
            return;
        }
    }

    if closing_active {
        tabs.persist_editor_into_active(ide_text, ide_baseline);
    }

    tabs.tabs.update(|list| {
        if index < list.len() {
            list.remove(index);
        }
    });

    let len = tabs.tabs.get_untracked().len();
    let prev_active = tabs.active.get_untracked();
    let new_active = if len == 0 {
        None
    } else if let Some(a) = prev_active {
        if a == index {
            Some(index.min(len.saturating_sub(1)))
        } else if a > index {
            Some(a - 1)
        } else {
            Some(a)
        }
    } else {
        None
    };

    tabs.active.set(new_active);
    tabs.load_active_into_editor(ide_path, ide_text, ide_baseline);
    if new_active.is_none() {
        tabs.err.set(None);
    }
}

/// 磁盘项已删除后强制关闭匹配的标签（不提示未保存；用户已在删除确认中授权）。
pub fn force_close_tabs_for_deleted_entry(
    tabs: IdeTabsHandle,
    deleted_rel: &str,
    is_dir: bool,
    editor: IdeTabsEditorSignals,
) {
    let IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    } = editor;
    tabs.persist_editor_into_active(ide_text, ide_baseline);
    let deleted = deleted_rel.trim().trim_end_matches('/');
    let dir_prefix = is_dir.then(|| format!("{deleted}/"));
    let should_remove = |path: &str| -> bool {
        if is_dir {
            path == deleted || dir_prefix.as_ref().is_some_and(|pfx| path.starts_with(pfx))
        } else {
            path == deleted
        }
    };
    let prev_active = tabs.active.get_untracked();
    let old_list = tabs.tabs.get_untracked();
    let mut new_list = Vec::with_capacity(old_list.len());
    let mut new_active = None;
    for (i, tab) in old_list.iter().enumerate() {
        if should_remove(tab.path.as_str()) {
            continue;
        }
        if prev_active == Some(i) {
            new_active = Some(new_list.len());
        }
        new_list.push(tab.clone());
    }
    tabs.tabs.set(new_list);
    tabs.active.set(new_active);
    tabs.load_active_into_editor(ide_path, ide_text, ide_baseline);
    if new_active.is_none() {
        tabs.err.set(None);
    }
}

pub fn make_ide_open_file_handler(
    locale: RwSignal<Locale>,
    tabs: IdeTabsHandle,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) -> Arc<dyn Fn(String) + Send + Sync> {
    let IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    } = editor;
    Arc::new(move |rel: String| {
        if tabs.load_busy.get_untracked() || tabs.save_busy.get_untracked() {
            return;
        }
        spawn_local(async move {
            if let Some(idx) = tabs.index_of_path(&rel) {
                let _ = try_switch_tab(tabs, idx, locale, editor, confirm).await;
                return;
            }
            if tabs.active_editor_is_dirty(ide_text, ide_baseline)
                && !confirm_discard(locale.get_untracked(), confirm).await
            {
                return;
            }
            tabs.persist_editor_into_active(ide_text, ide_baseline);
            tabs.load_busy.set(true);
            tabs.err.set(None);
            let loc = locale.get_untracked();
            let rel_c = rel.clone();
            match fetch_workspace_file(rel_c.as_str(), None, loc).await {
                Ok(d) => apply_fetch_result(tabs, d, rel_c, ide_path, ide_text, ide_baseline),
                Err(e) => apply_fetch_error(tabs, e, ide_path),
            }
            tabs.load_busy.set(false);
        });
    })
}

/// 关闭当前活动标签（快捷键等）；无活动标签时 no-op。
pub fn spawn_close_active_tab(
    tabs: IdeTabsHandle,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
    confirm: IdeConfirmSignals,
) {
    let Some(index) = tabs.active.get_untracked() else {
        return;
    };
    spawn_local(async move {
        close_tab_at(tabs, index, locale, editor, confirm).await;
    });
}

fn apply_fetch_result(
    tabs: IdeTabsHandle,
    d: WorkspaceFileReadData,
    rel: String,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
) {
    if let Some(e) = d.error {
        apply_fetch_error(tabs, e, ide_path);
        return;
    }
    apply_fetch_to_new_tab(tabs, rel, d.content, ide_path, ide_text, ide_baseline);
}
