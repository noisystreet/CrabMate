//! IDE 设置页顶栏。

use std::rc::Rc;

use leptos::prelude::*;

use crate::i18n::{self, Locale};

#[component]
pub(super) fn IdeSettingsPageHeader(
    locale: RwSignal<Locale>,
    dirty: Memo<bool>,
    on_back: Rc<dyn Fn()>,
    discard_rc: Rc<dyn Fn()>,
    save_rc: Rc<dyn Fn()>,
) -> impl IntoView {
    view! {
        <div class="settings-page-header">
            <button
                type="button"
                class="btn btn-ghost settings-page-back"
                on:click=move |_| on_back()
            >
                <svg
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    aria-hidden="true"
                >
                    <polyline points="15 18 9 12 15 6" />
                </svg>
                <span>{move || i18n::ide_settings_back(locale.get())}</span>
            </button>
            <h1 class="settings-page-title">{move || i18n::ide_settings_title(locale.get())}</h1>
            <span class="settings-page-badge">{move || i18n::ide_settings_badge_local(locale.get())}</span>
            <Show when=move || dirty.get()>
                <span class="settings-unsaved-pill">{move || i18n::ide_settings_unsaved_badge(locale.get())}</span>
            </Show>
            <span class="settings-page-head-spacer"></span>
            <div class="settings-page-header-actions">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:disabled=move || !dirty.get()
                    on:click=move |_| discard_rc()
                >
                    {move || i18n::ide_settings_discard_changes(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-primary btn-sm"
                    prop:disabled=move || !dirty.get()
                    on:click=move |_| save_rc()
                >
                    {move || i18n::ide_settings_save_all(locale.get())}
                </button>
            </div>
        </div>
    }
}
