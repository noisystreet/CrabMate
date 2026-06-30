//! IDE 打开标签与磁盘内容同步（Agent 工具写盘等外部变更）。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::fetch_workspace_file;
use crate::i18n::{self, Locale};
use crate::ide_confirm::{IdeConfirmSignals, ide_confirm_user};
use crate::ide_tabs::{IdeTabsEditorSignals, IdeTabsHandle};

/// 拉取磁盘内容并与各标签 `baseline` 比对；干净标签自动重载，脏标签经确认后重载。
pub fn spawn_sync_ide_tabs_from_disk(
    tabs: IdeTabsHandle,
    editor: IdeTabsEditorSignals,
    locale: RwSignal<Locale>,
    confirm: IdeConfirmSignals,
) {
    if tabs.load_busy.get_untracked() || tabs.save_busy.get_untracked() {
        return;
    }
    tabs.persist_editor_into_active(editor.ide_text, editor.ide_baseline);

    let active = tabs.active.get_untracked();
    let snapshot: Vec<(usize, String, String, String)> = tabs
        .tabs
        .get_untracked()
        .into_iter()
        .enumerate()
        .map(|(i, t)| (i, t.path.clone(), t.text.clone(), t.baseline.clone()))
        .collect();

    if snapshot.is_empty() {
        return;
    }

    spawn_local(async move {
        let loc = locale.get_untracked();
        let mut reloads: Vec<(usize, String, bool)> = Vec::new();
        for (idx, path, text, baseline) in snapshot {
            let Ok(data) = fetch_workspace_file(path.as_str(), None, loc).await else {
                continue;
            };
            if data.error.is_some() {
                continue;
            }
            if data.content == baseline {
                continue;
            }
            let dirty = text != baseline;
            if dirty
                && !ide_confirm_user(confirm, i18n::ide_disk_reload_confirm(loc).to_string()).await
            {
                continue;
            }
            reloads.push((idx, data.content, dirty));
        }

        for (idx, disk, _dirty) in reloads {
            tabs.tabs.update(|list| {
                if let Some(tab) = list.get_mut(idx) {
                    tab.text = disk.clone();
                    tab.baseline = disk.clone();
                }
            });
            if active == Some(idx) {
                editor
                    .ide_path
                    .set(tabs.tabs.get_untracked().get(idx).map(|t| t.path.clone()));
                editor.ide_text.set(disk.clone());
                editor.ide_baseline.set(disk);
            }
        }
    });
}
