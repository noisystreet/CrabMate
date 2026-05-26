//! 设置页顶栏（从 `view` 拆出以降低 `SettingsPageView` 的 nloc 棘轮）。

use std::rc::Rc;

use leptos::prelude::*;

use crate::i18n::{self, Locale};

#[component]
pub(super) fn SettingsPageHeader(
    appearance_locale: RwSignal<Locale>,
    dirty: Memo<bool>,
    on_back: Rc<dyn Fn()>,
    on_discard: Rc<dyn Fn()>,
    on_save: Rc<dyn Fn()>,
) -> impl IntoView {
    view! {
        <div class="settings-page-header">
            <button
                type="button"
                class="btn btn-ghost settings-page-back"
                data-testid="settings-back"
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
                <span>{move || i18n::settings_back(appearance_locale.get())}</span>
            </button>
            <h1 class="settings-page-title">{move || i18n::settings_title(appearance_locale.get())}</h1>
            <span class="settings-page-badge">{move || i18n::settings_badge_local(appearance_locale.get())}</span>
            <Show when=move || dirty.get()>
                <span class="settings-unsaved-pill">{move || i18n::settings_unsaved_badge(appearance_locale.get())}</span>
            </Show>
            <span class="settings-page-head-spacer"></span>
            <div class="settings-page-header-actions">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:disabled=move || !dirty.get()
                    on:click=move |_| on_discard()
                >
                    {move || i18n::settings_discard_changes(appearance_locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-primary btn-sm"
                    data-testid="settings-save-all"
                    prop:disabled=move || !dirty.get()
                    on:click=move |_| on_save()
                >
                    {move || i18n::settings_save_all(appearance_locale.get())}
                </button>
            </div>
        </div>
    }
}
