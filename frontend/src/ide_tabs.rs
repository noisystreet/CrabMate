//! IDE 多标签页：打开、切换、关闭与工作区文件加载。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{WorkspaceFileReadData, fetch_workspace_file};
use crate::i18n::{self, Locale};

/// 单个已打开文件的缓冲。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IdeTab {
    pub path: String,
    pub text: String,
    pub baseline: String,
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

fn confirm_discard(locale: Locale) -> bool {
    let msg = i18n::ide_dirty_confirm(locale);
    web_sys::window()
        .and_then(|w| w.confirm_with_message(msg).ok())
        .unwrap_or(false)
}

fn apply_fetch_to_new_tab(
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

pub fn try_switch_tab(
    tabs: IdeTabsHandle,
    index: usize,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
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
        && !confirm_discard(locale.get_untracked())
    {
        return false;
    }
    tabs.switch_to(index, ide_path, ide_text, ide_baseline);
    true
}

pub fn close_tab_at(
    tabs: IdeTabsHandle,
    index: usize,
    locale: RwSignal<Locale>,
    editor: IdeTabsEditorSignals,
) {
    let IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    } = editor;
    let closing_active = tabs.active.get_untracked() == Some(index);
    if closing_active && tabs.active_editor_is_dirty(ide_text, ide_baseline) {
        if !confirm_discard(locale.get_untracked()) {
            return;
        }
    } else if !closing_active {
        if let Some(tab) = tabs.tabs.get_untracked().get(index)
            && tab.text != tab.baseline
            && !confirm_discard(locale.get_untracked())
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

pub fn make_ide_open_file_handler(
    locale: RwSignal<Locale>,
    tabs: IdeTabsHandle,
    editor: IdeTabsEditorSignals,
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
        if let Some(idx) = tabs.index_of_path(&rel) {
            let _ = try_switch_tab(tabs, idx, locale, editor);
            return;
        }
        if tabs.active_editor_is_dirty(ide_text, ide_baseline)
            && !confirm_discard(locale.get_untracked())
        {
            return;
        }
        tabs.persist_editor_into_active(ide_text, ide_baseline);
        tabs.load_busy.set(true);
        tabs.err.set(None);
        let loc = locale.get_untracked();
        let rel_c = rel.clone();
        spawn_local(async move {
            match fetch_workspace_file(rel_c.as_str(), None, loc).await {
                Ok(d) => apply_fetch_result(tabs, d, rel_c, ide_path, ide_text, ide_baseline),
                Err(e) => apply_fetch_error(tabs, e, ide_path),
            }
            tabs.load_busy.set(false);
        });
    })
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
