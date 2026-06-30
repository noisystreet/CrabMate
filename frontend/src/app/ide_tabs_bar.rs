//! IDE 编辑器多标签页标签栏。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::i18n::{self, Locale};
use crate::ide_confirm::IdeConfirmSignals;
use crate::ide_tabs::{
    IdeTabsEditorSignals, IdeTabsHandle, close_tab_at, ide_tab_basename, try_switch_tab,
};

#[derive(Clone, Copy)]
pub struct IdeTabsBarInput {
    pub locale: RwSignal<Locale>,
    pub tabs: IdeTabsHandle,
    pub confirm: IdeConfirmSignals,
    pub editor: IdeTabsEditorSignals,
}

#[component]
pub fn IdeTabsBar(input: IdeTabsBarInput) -> impl IntoView {
    let IdeTabsBarInput {
        locale,
        tabs,
        confirm,
        editor,
    } = input;
    let IdeTabsEditorSignals {
        ide_path: _,
        ide_text,
        ide_baseline,
    } = editor;

    view! {
        <Show when=move || !tabs.tabs.get().is_empty()>
            <div
                class="ide-tabs-bar"
                role="tablist"
                prop:aria-label=move || i18n::ide_tabs_aria(locale.get())
            >
                <For
                    each=move || {
                        tabs.tabs
                            .get()
                            .into_iter()
                            .enumerate()
                            .collect::<Vec<_>>()
                    }
                    key=|(idx, tab)| format!("{}:{}", idx, tab.path)
                    children=move |(index, tab)| {
                        let path = tab.path.clone();
                        let label = ide_tab_basename(&path);
                        let is_active = move || tabs.active.get() == Some(index);
                        let is_dirty = move || {
                            tabs.tab_display_dirty(index, ide_text, ide_baseline)
                        };
                        view! {
                            <div
                                class="ide-tab"
                                class:ide-tab-active=is_active
                                role="presentation"
                            >
                                <button
                                    type="button"
                                    class="ide-tab-select"
                                    role="tab"
                                    prop:id=format!("ide-tab-{}", index)
                                    prop:aria-selected=move || is_active().to_string()
                                    prop:aria-controls="ide-editor-panel"
                                    prop:title=path.clone()
                                    data-testid=format!("ide-tab-{}", index)
                                    on:click=move |_| {
                                        spawn_local(async move {
                                            let _ = try_switch_tab(
                                                tabs, index, locale, editor, confirm,
                                            )
                                            .await;
                                        });
                                    }
                                >
                                    <Show when=is_dirty>
                                        <span class="ide-tab-dirty" aria-hidden="true">"●"</span>
                                    </Show>
                                    <span class="ide-tab-label">{label.clone()}</span>
                                </button>
                                <button
                                    type="button"
                                    class="ide-tab-close"
                                    prop:aria-label={
                                        let label = label.clone();
                                        move || i18n::ide_tab_close_aria(locale.get(), &label)
                                    }
                                    on:click=move |ev| {
                                        ev.stop_propagation();
                                        spawn_local(async move {
                                            close_tab_at(tabs, index, locale, editor, confirm)
                                                .await;
                                        });
                                    }
                                >
                                    <span aria-hidden="true">"×"</span>
                                </button>
                            </div>
                        }
                    }
                />
            </div>
        </Show>
    }
}
