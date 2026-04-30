use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::client_llm_presets::{CLIENT_LLM_API_BASE_PRESETS, preset_by_id};
use crate::i18n::{self, Locale};

#[component]
pub(crate) fn SettingsAppearanceBlock(
    locale: RwSignal<Locale>,
    appearance_locale: RwSignal<Locale>,
    appearance_theme: RwSignal<String>,
    appearance_bg_decor: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_language(locale.get())}</h3>
            <div class="settings-row">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    class:active=move || appearance_locale.get() == Locale::ZhHans
                    on:click=move |_| appearance_locale.set(Locale::ZhHans)
                >
                    {move || i18n::settings_lang_zh(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    class:active=move || appearance_locale.get() == Locale::En
                    on:click=move |_| appearance_locale.set(Locale::En)
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
                    class:active=move || appearance_theme.get() == "dark"
                    on:click=move |_| appearance_theme.set("dark".to_string())
                >
                    {move || i18n::settings_theme_dark(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    class:active=move || appearance_theme.get() == "light"
                    on:click=move |_| appearance_theme.set("light".to_string())
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
                    prop:checked=move || appearance_bg_decor.get()
                    on:change=move |_| appearance_bg_decor.update(|v| *v = !*v)
                />
                <span>{move || i18n::settings_bg_glow(locale.get())}</span>
            </label>
        </div>
    }
}

#[component]
pub(crate) fn SettingsLlmBlock(
    locale: RwSignal<Locale>,
    llm_api_base_draft: RwSignal<String>,
    llm_api_base_preset_select: RwSignal<String>,
    llm_model_draft: RwSignal<String>,
    llm_temperature_draft: RwSignal<String>,
    execution_mode_draft: Option<RwSignal<String>>,
    llm_api_key_draft: RwSignal<String>,
    llm_has_saved_key: RwSignal<bool>,
    clear_client_key_intent: RwSignal<bool>,
    hint_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_llm(locale.get())}</h3>
            <p class=hint_class>{move || i18n::settings_llm_hint(locale.get())}</p>
            <div class="settings-field">
                <label class="settings-field-label" for="settings-llm-api-base-preset">
                    {move || i18n::settings_label_api_base_preset(locale.get())}
                </label>
                <select
                    id="settings-llm-api-base-preset"
                    class="settings-select"
                    prop:value=move || llm_api_base_preset_select.get()
                    on:change=move |ev| {
                        let id = event_target_value(&ev);
                        llm_api_base_preset_select.set(id.clone());
                        let Some(p) = preset_by_id(id.as_str()) else {
                            return;
                        };
                        if p.id == "custom" {
                            return;
                        }
                        llm_api_base_draft.set(p.url.to_string());
                        if let Some(m) = p.suggested_model
                            && llm_model_draft.get_untracked().trim().is_empty()
                        {
                            llm_model_draft.set(m.to_string());
                        }
                    }
                >
                    {CLIENT_LLM_API_BASE_PRESETS
                        .iter()
                        .filter(|p| p.id != "custom")
                        .map(|p| {
                            let id = p.id;
                            view! {
                                <option value=id>{move || i18n::settings_api_base_preset_label(id, locale.get())}</option>
                            }
                        })
                        .collect_view()}
                    <option value="custom">{move || i18n::settings_api_base_preset_custom(locale.get())}</option>
                </select>
            </div>
            <Show when=move || llm_api_base_preset_select.get() == "custom">
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
                            llm_api_base_preset_select.set("custom".to_string());
                            llm_api_base_draft.set(event_target_value(&ev));
                        }
                    />
                </div>
            </Show>
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
                    on:input=move |ev| llm_model_draft.set(event_target_value(&ev))
                />
            </div>
            <div class="settings-field">
                <label class="settings-field-label" for="settings-llm-temperature">
                    {move || i18n::settings_label_temperature(locale.get())}
                </label>
                <input
                    type="number"
                    id="settings-llm-temperature"
                    class="settings-text-input"
                    min="0"
                    max="2"
                    step="0.1"
                    prop:placeholder=move || i18n::settings_ph_temperature(locale.get())
                    prop:value=move || llm_temperature_draft.get()
                    on:input=move |ev| llm_temperature_draft.set(event_target_value(&ev))
                />
                <p class=hint_class>{move || i18n::settings_temperature_hint(locale.get())}</p>
            </div>
            <Show when=move || execution_mode_draft.is_some()>
                <div class="settings-field">
                    <label class="settings-field-label" for="settings-execution-mode">
                        {move || i18n::settings_label_execution_mode(locale.get())}
                    </label>
                    <select
                        id="settings-execution-mode"
                        class="settings-select"
                        prop:value=move || execution_mode_draft.map(|s| s.get()).unwrap_or_default()
                        on:change=move |ev| {
                            if let Some(sig) = execution_mode_draft {
                                sig.set(event_target_value(&ev));
                            }
                        }
                    >
                        <option value="rolling_planning">
                            {move || i18n::settings_execution_mode_rolling(locale.get())}
                        </option>
                        <option value="hierarchical">
                            {move || i18n::settings_execution_mode_hierarchical(locale.get())}
                        </option>
                    </select>
                    <p class=hint_class>{move || i18n::settings_execution_mode_hint(locale.get())}</p>
                </div>
            </Show>
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
                    on:input=move |ev| llm_api_key_draft.set(event_target_value(&ev))
                />
            </div>
            <Show when=move || llm_has_saved_key.get() && !clear_client_key_intent.get()>
                <p class=hint_class>{move || i18n::settings_key_saved_note(locale.get())}</p>
            </Show>
            <div class="settings-actions-row">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:disabled=move || !llm_has_saved_key.get() || clear_client_key_intent.get()
                    on:click=move |_| {
                        clear_client_key_intent.set(true);
                        llm_api_key_draft.set(String::new());
                    }
                >
                    {move || i18n::settings_clear_key(locale.get())}
                </button>
            </div>
        </div>
    }
}

#[component]
pub(crate) fn SettingsExecutorLlmBlock(
    locale: RwSignal<Locale>,
    executor_llm_api_base_draft: RwSignal<String>,
    executor_llm_api_base_preset_select: RwSignal<String>,
    executor_llm_model_draft: RwSignal<String>,
    executor_llm_api_key_draft: RwSignal<String>,
    executor_llm_has_saved_key: RwSignal<bool>,
    clear_executor_key_intent: RwSignal<bool>,
    hint_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_executor_llm(locale.get())}</h3>
            <p class=hint_class>{move || i18n::settings_executor_llm_hint(locale.get())}</p>
            <div class="settings-field">
                <label class="settings-field-label" for="settings-executor-llm-api-base-preset">
                    {move || i18n::settings_label_api_base_preset(locale.get())}
                </label>
                <select
                    id="settings-executor-llm-api-base-preset"
                    class="settings-select"
                    prop:value=move || executor_llm_api_base_preset_select.get()
                    on:change=move |ev| {
                        let id = event_target_value(&ev);
                        executor_llm_api_base_preset_select.set(id.clone());
                        let Some(p) = preset_by_id(id.as_str()) else {
                            return;
                        };
                        if p.id == "custom" {
                            return;
                        }
                        executor_llm_api_base_draft.set(p.url.to_string());
                        if let Some(m) = p.suggested_model
                            && executor_llm_model_draft.get_untracked().trim().is_empty()
                        {
                            executor_llm_model_draft.set(m.to_string());
                        }
                    }
                >
                    {CLIENT_LLM_API_BASE_PRESETS
                        .iter()
                        .filter(|p| p.id != "custom")
                        .map(|p| {
                            let id = p.id;
                            view! {
                                <option value=id>{move || i18n::settings_api_base_preset_label(id, locale.get())}</option>
                            }
                        })
                        .collect_view()}
                    <option value="custom">{move || i18n::settings_api_base_preset_custom(locale.get())}</option>
                </select>
            </div>
            <Show when=move || executor_llm_api_base_preset_select.get() == "custom">
                <div class="settings-field">
                    <label class="settings-field-label" for="settings-executor-llm-api-base">
                        {move || i18n::settings_label_executor_api_base(locale.get())}
                    </label>
                    <input
                        type="text"
                        id="settings-executor-llm-api-base"
                        class="settings-text-input"
                        prop:placeholder=move || i18n::settings_ph_api_base(locale.get())
                        prop:value=move || executor_llm_api_base_draft.get()
                        on:input=move |ev| {
                            executor_llm_api_base_preset_select.set("custom".to_string());
                            executor_llm_api_base_draft.set(event_target_value(&ev));
                        }
                    />
                </div>
            </Show>
            <div class="settings-field">
                <label class="settings-field-label" for="settings-executor-llm-model">
                    {move || i18n::settings_label_executor_model(locale.get())}
                </label>
                <input
                    type="text"
                    id="settings-executor-llm-model"
                    class="settings-text-input"
                    prop:placeholder=move || i18n::settings_ph_model(locale.get())
                    prop:value=move || executor_llm_model_draft.get()
                    on:input=move |ev| executor_llm_model_draft.set(event_target_value(&ev))
                />
            </div>
            <div class="settings-field">
                <label class="settings-field-label" for="settings-executor-llm-api-key">
                    {move || i18n::settings_label_executor_api_key(locale.get())}
                </label>
                <input
                    type="password"
                    id="settings-executor-llm-api-key"
                    class="settings-text-input"
                    autocomplete="off"
                    prop:placeholder=move || i18n::settings_ph_executor_api_key(locale.get())
                    prop:value=move || executor_llm_api_key_draft.get()
                    on:input=move |ev| executor_llm_api_key_draft.set(event_target_value(&ev))
                />
            </div>
            <Show when=move || executor_llm_has_saved_key.get() && !clear_executor_key_intent.get()>
                <p class=hint_class>{move || i18n::settings_executor_key_saved_note(locale.get())}</p>
            </Show>
            <div class="settings-actions-row">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    prop:disabled=move || !executor_llm_has_saved_key.get() || clear_executor_key_intent.get()
                    on:click=move |_| {
                        clear_executor_key_intent.set(true);
                        executor_llm_api_key_draft.set(String::new());
                    }
                >
                    {move || i18n::settings_clear_executor_key(locale.get())}
                </button>
            </div>
        </div>
    }
}

#[component]
pub(crate) fn SettingsShortcutsBlock(
    locale: RwSignal<Locale>,
    body_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_shortcuts(locale.get())}</h3>
            <p class=body_class>{move || i18n::settings_shortcuts_body(locale.get())}</p>
        </div>
    }
}
