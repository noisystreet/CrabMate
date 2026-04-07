//! 设置：主题、背景、本机模型覆盖。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};
use crate::api::{
    clear_client_llm_api_key_storage, client_llm_storage_has_api_key, persist_client_llm_to_storage,
};
use crate::i18n::{self, Locale, store_locale_slug};

#[allow(clippy::too_many_arguments)]
pub fn settings_modal_view(
    settings_modal: RwSignal<bool>,
    locale: RwSignal<Locale>,
    theme: RwSignal<String>,
    bg_decor: RwSignal<bool>,
    #[allow(unused_variables)] status_data: RwSignal<Option<crate::api::StatusData>>,
    llm_api_base_draft: RwSignal<String>,
    llm_model_draft: RwSignal<String>,
    llm_api_key_draft: RwSignal<String>,
    llm_has_saved_key: RwSignal<bool>,
    llm_settings_feedback: RwSignal<Option<String>>,
    client_llm_storage_tick: RwSignal<u64>,
) -> impl IntoView {
    let settings_dialog_ref = NodeRef::<Div>::new();

    Effect::new({
        let settings_dialog_ref = settings_dialog_ref.clone();
        move |_| {
            if !settings_modal.get() {
                return;
            }
            let r = settings_dialog_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = r.get() {
                    focus_first_in_modal_container(el.as_ref());
                }
            });
        }
    });

    view! {
            <Show when=move || settings_modal.get()>
                <div class="modal-backdrop" on:click=move |_| settings_modal.set(false)>
                    <div
                        class="modal"
                        node_ref=settings_dialog_ref
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="settings-modal-title"
                        tabindex="-1"
                        on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Tab" {
                                if let Some(el) = settings_dialog_ref.get() {
                                    trap_tab_in_container(&ev, el.as_ref());
                                }
                            }
                        }
                    >
                        <div class="modal-head">
                            <h2 class="modal-title" id="settings-modal-title">{move || i18n::settings_title(locale.get())}</h2>
                            <span class="modal-badge">{move || i18n::settings_badge_local(locale.get())}</span>
                            <span class="modal-head-spacer"></span>
                            <button type="button" class="btn btn-ghost btn-sm" on:click=move |_| settings_modal.set(false)>
                                {move || i18n::settings_close(locale.get())}
                            </button>
                        </div>
                        <div class="modal-body">
                            <p class="modal-hint">{move || i18n::settings_intro(locale.get())}</p>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_language(locale.get())}</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || locale.get() == Locale::ZhHans
                                        on:click=move |_| {
                                            locale.set(Locale::ZhHans);
                                            store_locale_slug(Locale::ZhHans.storage_slug());
                                        }
                                    >
                                        {move || i18n::settings_lang_zh(locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || locale.get() == Locale::En
                                        on:click=move |_| {
                                            locale.set(Locale::En);
                                            store_locale_slug(Locale::En.storage_slug());
                                        }
                                    >
                                        {move || i18n::settings_lang_en(locale.get())}
                                    </button>
                                </div>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_theme(locale.get())}</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "dark"
                                        on:click=move |_| theme.set("dark".to_string())
                                    >
                                        {move || i18n::settings_theme_dark(locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "light"
                                        on:click=move |_| theme.set("light".to_string())
                                    >
                                        {move || i18n::settings_theme_light(locale.get())}
                                    </button>
                                </div>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_bg(locale.get())}</h3>
                                <label class="settings-checkbox-label">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || bg_decor.get()
                                        on:change=move |_| bg_decor.update(|v| *v = !*v)
                                    />
                                    <span>{move || i18n::settings_bg_glow(locale.get())}</span>
                                </label>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_llm(locale.get())}</h3>
                                <p class="modal-hint settings-field-nested-hint">
                                    {move || i18n::settings_llm_hint(locale.get())}
                                </p>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-base">
                                        {move || i18n::settings_label_api_base(locale.get())}
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-api-base"
                                        class="settings-text-input"
                                        prop:placeholder=move || i18n::settings_ph_api_base(locale.get())
                                        prop:value=move || llm_api_base_draft.get()
                                        on:input=move |ev| {
                                            llm_api_base_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-model">
                                        {move || i18n::settings_label_model(locale.get())}
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-model"
                                        class="settings-text-input"
                                        prop:placeholder=move || i18n::settings_ph_model(locale.get())
                                        prop:value=move || llm_model_draft.get()
                                        on:input=move |ev| {
                                            llm_model_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-key">
                                        {move || i18n::settings_label_api_key(locale.get())}
                                    </label>
                                    <input
                                        type="password"
                                        id="settings-llm-api-key"
                                        class="settings-text-input"
                                        autocomplete="off"
                                        prop:placeholder=move || i18n::settings_ph_api_key(locale.get())
                                        prop:value=move || llm_api_key_draft.get()
                                        on:input=move |ev| {
                                            llm_api_key_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <Show when=move || llm_has_saved_key.get()>
                                    <p class="modal-hint settings-field-nested-hint">
                                        {move || i18n::settings_key_saved_note(locale.get())}
                                    </p>
                                </Show>
                                <div class="settings-actions-row">
                                    <button
                                        type="button"
                                        class="btn btn-primary btn-sm"
                                        on:click=move |_| {
                                            llm_settings_feedback.set(None);
                                            let loc = locale.get_untracked();
                                            let key_raw = llm_api_key_draft.get();
                                            let api_key_upd = if key_raw.trim().is_empty() {
                                                None
                                            } else {
                                                Some(key_raw)
                                            };
                                            let base = llm_api_base_draft.get();
                                            let model = llm_model_draft.get();
                                            match persist_client_llm_to_storage(
                                                &base,
                                                &model,
                                                api_key_upd.as_deref(),
                                                loc,
                                            ) {
                                                Ok(()) => {
                                                    llm_api_key_draft.set(String::new());
                                                    llm_has_saved_key
                                                        .set(client_llm_storage_has_api_key());
                                                    client_llm_storage_tick
                                                        .update(|n| *n = n.wrapping_add(1));
                                                    llm_settings_feedback.set(Some(
                                                        i18n::settings_saved_browser(loc).to_string(),
                                                    ));
                                                }
                                                Err(e) => llm_settings_feedback.set(Some(e)),
                                            }
                                        }
                                    >
                                        {move || i18n::settings_save_llm(locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        prop:disabled=move || !llm_has_saved_key.get()
                                        on:click=move |_| {
                                            llm_settings_feedback.set(None);
                                            let loc = locale.get_untracked();
                                            let _ = clear_client_llm_api_key_storage(loc);
                                            llm_has_saved_key.set(false);
                                            llm_settings_feedback.set(Some(
                                                i18n::settings_cleared_key(loc).to_string(),
                                            ));
                                        }
                                    >
                                        {move || i18n::settings_clear_key(locale.get())}
                                    </button>
                                </div>
                                <Show when=move || llm_settings_feedback.get().is_some()>
                                    <p class="settings-save-feedback">{move || {
                                        llm_settings_feedback.get().unwrap_or_default()
                                    }}</p>
                                </Show>
                            </div>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_shortcuts(locale.get())}</h3>
                                <p class="modal-hint">{move || i18n::settings_shortcuts_body(locale.get())}</p>
                            </div>
                        </div>
                    </div>
                </div>
            </Show>
    }
}
