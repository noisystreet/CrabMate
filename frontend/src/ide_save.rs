//! IDE 编辑器：保存当前标签、全部脏标签与工作区新建文件。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{post_workspace_file_write, post_workspace_file_write_opts};
use crate::i18n::{self, Locale};
use crate::ide_tabs::IdeTabsHandle;

/// 保存/新建时共用的编辑器与标签信号。
#[derive(Clone, Copy)]
pub struct IdeSaveContext {
    pub tabs: IdeTabsHandle,
    pub ide_path: RwSignal<Option<String>>,
    pub ide_text: RwSignal<String>,
    pub ide_baseline: RwSignal<String>,
    pub ide_err: RwSignal<Option<String>>,
}

/// 将成功写入的快照同步到活动标签与编辑器缓冲。
pub fn apply_saved_snapshot(tabs: IdeTabsHandle, ide_baseline: RwSignal<String>, snap: String) {
    ide_baseline.set(snap.clone());
    if let Some(i) = tabs.active.get_untracked() {
        tabs.tabs.update(|list| {
            if let Some(tab) = list.get_mut(i) {
                tab.text = snap.clone();
                tab.baseline = snap;
            }
        });
    }
}

/// 异步保存指定路径内容；`update_only` 为 true 时要求文件已存在。
pub async fn save_path_content(
    path: String,
    content: String,
    update_only: bool,
    loc: Locale,
) -> Result<(), String> {
    if update_only {
        post_workspace_file_write_opts(path, content, false, true, loc).await
    } else {
        post_workspace_file_write(path, content, loc).await
    }
}

fn spawn_save_one(ctx: IdeSaveContext, path: String, content: String, locale: RwSignal<Locale>) {
    if ctx.tabs.load_busy.get_untracked() || ctx.tabs.save_busy.get_untracked() {
        return;
    }
    ctx.tabs.save_busy.set(true);
    ctx.ide_err.set(None);
    spawn_local(async move {
        let loc = locale.get_untracked();
        match save_path_content(path, content, true, loc).await {
            Ok(()) => {
                let snap = ctx.ide_text.get_untracked();
                apply_saved_snapshot(ctx.tabs, ctx.ide_baseline, snap);
            }
            Err(e) => ctx.ide_err.set(Some(e)),
        }
        ctx.tabs.save_busy.set(false);
    });
}

/// 保存当前活动标签（无脏改动或无路径时 no-op）。
pub fn spawn_save_active_tab(ctx: IdeSaveContext, locale: RwSignal<Locale>) {
    let Some(path) = ctx.ide_path.get_untracked() else {
        return;
    };
    if ctx.ide_text.get_untracked() == ctx.ide_baseline.get_untracked() {
        return;
    }
    ctx.tabs
        .persist_editor_into_active(ctx.ide_text, ctx.ide_baseline);
    let body = ctx.ide_text.get_untracked();
    spawn_save_one(ctx, path, body, locale);
}

/// 保存全部未保存标签（含非活动标签缓冲）。
pub fn spawn_save_all_dirty_tabs(ctx: IdeSaveContext, locale: RwSignal<Locale>) {
    if ctx.tabs.load_busy.get_untracked() || ctx.tabs.save_busy.get_untracked() {
        return;
    }
    ctx.tabs
        .persist_editor_into_active(ctx.ide_text, ctx.ide_baseline);

    let active = ctx.tabs.active.get_untracked();
    let mut jobs: Vec<(String, String)> = ctx
        .tabs
        .tabs
        .get_untracked()
        .into_iter()
        .enumerate()
        .filter_map(|(i, tab)| {
            if tab.text == tab.baseline {
                return None;
            }
            let text = if active == Some(i) {
                ctx.ide_text.get_untracked()
            } else {
                tab.text
            };
            Some((tab.path, text))
        })
        .collect();

    if jobs.is_empty() {
        return;
    }

    ctx.tabs.save_busy.set(true);
    ctx.ide_err.set(None);
    spawn_local(async move {
        let loc = locale.get_untracked();
        let mut first_err: Option<String> = None;
        for (path, content) in jobs.drain(..) {
            if let Err(e) = save_path_content(path, content, true, loc).await {
                first_err = Some(e);
                break;
            }
        }
        if let Some(e) = first_err {
            ctx.ide_err.set(Some(e));
        } else {
            ctx.tabs.tabs.update(|list| {
                for tab in list.iter_mut() {
                    tab.baseline = tab.text.clone();
                }
            });
            if let Some(i) = ctx.tabs.active.get_untracked() {
                if let Some(tab) = ctx.tabs.tabs.get_untracked().get(i) {
                    ctx.ide_baseline.set(tab.baseline.clone());
                }
            }
        }
        ctx.tabs.save_busy.set(false);
    });
}

/// 浏览器 `prompt` 收集新建相对路径；空白或取消返回 `None`。
#[must_use]
pub fn prompt_new_workspace_file_path(locale: Locale) -> Option<String> {
    let raw = web_sys::window()
        .and_then(|w| {
            w.prompt_with_message_and_default(i18n::ide_new_file_prompt(locale), "")
                .ok()
        })
        .flatten()?;
    let path = raw.trim().to_string();
    if path.is_empty() || path.chars().any(|c| c.is_whitespace()) {
        return None;
    }
    Some(path)
}

/// 在工作区创建空文件并打开为新标签；成功后可选调用 `after_create`（例如刷新侧栏树）。
pub fn spawn_create_and_open_file(
    ctx: IdeSaveContext,
    locale: RwSignal<Locale>,
    rel: String,
    after_create: Option<Arc<dyn Fn() + Send + Sync>>,
) {
    if ctx.tabs.load_busy.get_untracked() || ctx.tabs.save_busy.get_untracked() {
        return;
    }
    if ctx
        .tabs
        .active_editor_is_dirty(ctx.ide_text, ctx.ide_baseline)
    {
        let msg = i18n::ide_dirty_confirm(locale.get_untracked());
        let discard = web_sys::window()
            .and_then(|w| w.confirm_with_message(msg).ok())
            .unwrap_or(false);
        if !discard {
            return;
        }
    }
    ctx.tabs
        .persist_editor_into_active(ctx.ide_text, ctx.ide_baseline);
    ctx.tabs.load_busy.set(true);
    ctx.ide_err.set(None);
    let loc = locale.get_untracked();
    spawn_local(async move {
        match post_workspace_file_write_opts(rel.clone(), String::new(), true, false, loc).await {
            Ok(()) => {
                crate::ide_tabs::apply_fetch_to_new_tab(
                    ctx.tabs,
                    rel,
                    String::new(),
                    ctx.ide_path,
                    ctx.ide_text,
                    ctx.ide_baseline,
                );
                if let Some(f) = after_create {
                    f();
                }
            }
            Err(e) => ctx.ide_err.set(Some(e)),
        }
        ctx.tabs.load_busy.set(false);
    });
}
