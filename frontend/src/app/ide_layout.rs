//! 主区内 IDE 布局：左侧工作区树 + 右侧文本编辑器（`GET/POST /workspace/file`）。

use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::api::{WorkspaceFileReadData, fetch_workspace_file, post_workspace_file_write};
use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};

use super::side_column_workspace_scroll::WorkspaceSideCardScrollInner;
use super::workspace_panel_state::WorkspacePanelSignals;

fn apply_workspace_file_read(
    d: WorkspaceFileReadData,
    rel_c: String,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
    ide_err: RwSignal<Option<String>>,
) {
    if let Some(e) = d.error {
        ide_err.set(Some(e));
        ide_path.set(None);
        ide_text.set(String::new());
        ide_baseline.set(String::new());
    } else {
        ide_path.set(Some(rel_c));
        ide_text.set(d.content.clone());
        ide_baseline.set(d.content);
    }
}

fn make_ide_open_file_handler(
    locale: RwSignal<Locale>,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
    ide_load_busy: RwSignal<bool>,
    ide_save_busy: RwSignal<bool>,
    ide_err: RwSignal<Option<String>>,
) -> Arc<dyn Fn(String) + Send + Sync> {
    Arc::new(move |rel: String| {
        if ide_load_busy.get_untracked() || ide_save_busy.get_untracked() {
            return;
        }
        if ide_text.get_untracked() != ide_baseline.get_untracked() {
            let msg = i18n::ide_dirty_confirm(locale.get_untracked());
            let ok = web_sys::window()
                .and_then(|w| w.confirm_with_message(msg).ok())
                .unwrap_or(false);
            if !ok {
                return;
            }
        }
        ide_load_busy.set(true);
        ide_err.set(None);
        let loc = locale.get_untracked();
        let rel_c = rel.clone();
        spawn_local(async move {
            match fetch_workspace_file(rel_c.as_str(), None, loc).await {
                Ok(d) => {
                    apply_workspace_file_read(d, rel_c, ide_path, ide_text, ide_baseline, ide_err)
                }
                Err(e) => {
                    ide_err.set(Some(e));
                    ide_path.set(None);
                }
            }
            ide_load_busy.set(false);
        });
    })
}

#[component]
fn IdeLayoutLeftPane(
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    workspace_panel: WorkspacePanelSignals,
    noop_sv: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    open_sv: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    view! {
        <div class="ide-layout-left">
            <div class="ide-layout-left-head">
                <div class="ide-pane-title">{move || i18n::ide_workspace_title(locale.get())}</div>
                <p class="ide-open-hint">{move || i18n::ide_open_hint(locale.get())}</p>
            </div>
            <div class="ide-workspace-scroll">
                <WorkspaceSideCardScrollInner
                    locale=locale
                    chat=chat
                    ws=workspace_panel
                    insert_workspace_file_ref=noop_sv
                    on_file_single_click=open_sv
                />
            </div>
        </div>
    }
}

#[component]
fn IdeLayoutEditorToolbar(
    locale: RwSignal<Locale>,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
    ide_load_busy: RwSignal<bool>,
    ide_save_busy: RwSignal<bool>,
    ide_err: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="ide-editor-toolbar">
            <div class="ide-editor-toolbar-title">
                {move || i18n::ide_editor_title(locale.get())}
                {move || {
                    if ide_path.get().is_none() {
                        String::new()
                    } else {
                        format!(" · {}", ide_path.get().unwrap_or_default())
                    }
                }}
            </div>
            <div class="ide-editor-toolbar-actions">
                <span class="ide-dirty-dot" aria-hidden="true">{move || {
                    if ide_text.get() != ide_baseline.get() {
                        "●"
                    } else {
                        ""
                    }
                }}</span>
                <button
                    type="button"
                    class="btn btn-primary btn-sm"
                    prop:disabled=move || {
                        ide_path.get().is_none()
                            || ide_load_busy.get()
                            || ide_save_busy.get()
                            || ide_text.get() == ide_baseline.get()
                    }
                    on:click=move |_| {
                        let Some(p) = ide_path.get_untracked() else {
                            return;
                        };
                        ide_save_busy.set(true);
                        ide_err.set(None);
                        let body = ide_text.get_untracked();
                        let loc = locale.get_untracked();
                        spawn_local(async move {
                            match post_workspace_file_write(p, body, loc).await {
                                Ok(()) => {
                                    let snap = ide_text.get_untracked();
                                    ide_baseline.set(snap);
                                }
                                Err(e) => ide_err.set(Some(e)),
                            }
                            ide_save_busy.set(false);
                        });
                    }
                >
                    {move || {
                        if ide_save_busy.get() {
                            i18n::ide_saving(locale.get())
                        } else {
                            i18n::ide_save(locale.get())
                        }
                    }}
                </button>
            </div>
        </div>
    }
}

#[component]
fn IdeLayoutRightPane(
    locale: RwSignal<Locale>,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
    ide_load_busy: RwSignal<bool>,
    ide_save_busy: RwSignal<bool>,
    ide_err: RwSignal<Option<String>>,
) -> impl IntoView {
    view! {
        <div class="ide-layout-right">
            <IdeLayoutEditorToolbar
                locale=locale
                ide_path=ide_path
                ide_text=ide_text
                ide_baseline=ide_baseline
                ide_load_busy=ide_load_busy
                ide_save_busy=ide_save_busy
                ide_err=ide_err
            />
            <Show when=move || ide_err.get().is_some()>
                <div class="msg-error ide-editor-err">{move || ide_err.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || ide_load_busy.get()>
                <p class="ide-editor-loading" role="status">"…"</p>
            </Show>
            <textarea
                class="ide-editor-textarea"
                prop:spellcheck="false"
                prop:placeholder=move || {
                    if ide_path.get().is_none() {
                        i18n::ide_no_file(locale.get())
                    } else {
                        ""
                    }
                }
                prop:disabled=move || ide_path.get().is_none() || ide_load_busy.get()
                bind:value=ide_text
            />
        </div>
    }
}

#[component]
pub fn IdeLayoutView(
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    workspace_panel: WorkspacePanelSignals,
    refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    initialized: RwSignal<bool>,
    /// 与主壳「编辑器布局」一致；不可见时不触发工作区刷新，避免后台无意义请求。
    editor_visible: RwSignal<bool>,
) -> impl IntoView {
    let ide_path = RwSignal::new(None::<String>);
    let ide_text = RwSignal::new(String::new());
    let ide_baseline = RwSignal::new(String::new());
    let ide_load_busy = RwSignal::new(false);
    let ide_save_busy = RwSignal::new(false);
    let ide_err = RwSignal::new(None::<String>);

    let noop: Arc<dyn Fn(String) + Send + Sync> = Arc::new(|_| {});
    let noop_sv = StoredValue::new(noop);

    let refresh_ws = refresh_workspace.clone();
    Effect::new(move |_| {
        if !editor_visible.get() {
            return;
        }
        if initialized.get() {
            refresh_ws();
        }
    });

    let open_file = make_ide_open_file_handler(
        locale,
        ide_path,
        ide_text,
        ide_baseline,
        ide_load_busy,
        ide_save_busy,
        ide_err,
    );
    let open_sv = StoredValue::new(open_file);

    view! {
        <div class="ide-layout-root" data-testid="ide-layout-root">
            <IdeLayoutLeftPane
                locale=locale
                chat=chat
                workspace_panel=workspace_panel
                noop_sv=noop_sv
                open_sv=open_sv
            />
            <IdeLayoutRightPane
                locale=locale
                ide_path=ide_path
                ide_text=ide_text
                ide_baseline=ide_baseline
                ide_load_busy=ide_load_busy
                ide_save_busy=ide_save_busy
                ide_err=ide_err
            />
        </div>
    }
}
