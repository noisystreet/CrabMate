//! 设置页 LLM 表单字段子组件（降低 `settings_sections` 中单组件圈复杂度）。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::client_llm_presets::{CLIENT_LLM_API_BASE_PRESETS, preset_by_id};
use crate::i18n::{self, Locale};

#[component]
pub(crate) fn LlmApiBasePresetSelect(
    locale: RwSignal<Locale>,
    api_base_draft: RwSignal<String>,
    api_base_preset_select: RwSignal<String>,
    model_draft: RwSignal<String>,
    select_id: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=select_id>
                {move || i18n::settings_label_api_base_preset(locale.get())}
            </label>
            <select
                id=select_id
                class="settings-select"
                prop:value=move || api_base_preset_select.get()
                on:change=move |ev| {
                    let id = event_target_value(&ev);
                    api_base_preset_select.set(id.clone());
                    let Some(p) = preset_by_id(id.as_str()) else {
                        return;
                    };
                    if p.id == "custom" {
                        return;
                    }
                    api_base_draft.set(p.url.to_string());
                    if let Some(m) = p.suggested_model
                        && model_draft.get_untracked().trim().is_empty()
                    {
                        model_draft.set(m.to_string());
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
    }
}

#[component]
pub(crate) fn LlmCustomApiBaseInput(
    locale: RwSignal<Locale>,
    api_base_draft: RwSignal<String>,
    api_base_preset_select: RwSignal<String>,
    label_fn: fn(Locale) -> &'static str,
    placeholder_fn: fn(Locale) -> &'static str,
    input_id: &'static str,
) -> impl IntoView {
    view! {
        <Show when=move || api_base_preset_select.get() == "custom">
            <div class="settings-field">
                <label class="settings-field-label" for=input_id>
                    {move || label_fn(locale.get())}
                </label>
                <input
                    type="text"
                    id=input_id
                    class="settings-text-input"
                    prop:placeholder=move || placeholder_fn(locale.get())
                    prop:value=move || api_base_draft.get()
                    on:input=move |ev| {
                        api_base_preset_select.set("custom".to_string());
                        api_base_draft.set(event_target_value(&ev));
                    }
                />
            </div>
        </Show>
    }
}

#[component]
pub(crate) fn LlmModelField(
    locale: RwSignal<Locale>,
    model_draft: RwSignal<String>,
    label_fn: fn(Locale) -> &'static str,
    placeholder_fn: fn(Locale) -> &'static str,
    input_id: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=input_id>
                {move || label_fn(locale.get())}
            </label>
            <input
                type="text"
                id=input_id
                class="settings-text-input"
                prop:placeholder=move || placeholder_fn(locale.get())
                prop:value=move || model_draft.get()
                on:input=move |ev| model_draft.set(event_target_value(&ev))
            />
        </div>
    }
}

#[component]
pub(crate) fn LlmContextTokensField(
    locale: RwSignal<Locale>,
    llm_context_tokens_draft: RwSignal<String>,
    hint_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for="settings-llm-context-tokens">
                {move || i18n::settings_label_llm_context_tokens(locale.get())}
            </label>
            <input
                type="number"
                id="settings-llm-context-tokens"
                class="settings-text-input"
                min="0"
                max="10000000"
                step="1"
                inputmode="numeric"
                prop:placeholder=move || i18n::settings_ph_llm_context_tokens(locale.get())
                prop:value=move || llm_context_tokens_draft.get()
                on:input=move |ev| llm_context_tokens_draft.set(leptos_dom::helpers::event_target_value(&ev))
            />
            <p class=hint_class>{move || i18n::settings_llm_context_tokens_hint(locale.get())}</p>
        </div>
    }
}

#[component]
pub(crate) fn LlmTemperatureField(
    locale: RwSignal<Locale>,
    temperature_draft: RwSignal<String>,
    hint_class: &'static str,
) -> impl IntoView {
    view! {
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
                step="any"
                inputmode="decimal"
                prop:placeholder=move || i18n::settings_ph_temperature(locale.get())
                prop:value=move || temperature_draft.get()
                on:input=move |ev| temperature_draft.set(event_target_value(&ev))
            />
            <p class=hint_class>{move || i18n::settings_temperature_hint(locale.get())}</p>
        </div>
    }
}

#[component]
pub(crate) fn LlmExecutionModeField(
    locale: RwSignal<Locale>,
    execution_mode_draft: RwSignal<String>,
    hint_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for="settings-execution-mode">
                {move || i18n::settings_label_execution_mode(locale.get())}
            </label>
            <select
                id="settings-execution-mode"
                class="settings-select"
                prop:value=move || execution_mode_draft.get()
                on:change=move |ev| execution_mode_draft.set(event_target_value(&ev))
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
    }
}

#[component]
pub(crate) fn OptionalLlmExecutionModeField(
    locale: RwSignal<Locale>,
    execution_mode_draft: Option<RwSignal<String>>,
    hint_class: &'static str,
) -> impl IntoView {
    match execution_mode_draft {
        Some(sig) => view! { <LlmExecutionModeField locale execution_mode_draft=sig hint_class /> }
            .into_any(),
        None => view! { <></> }.into_any(),
    }
}

#[component]
pub(crate) fn LlmThinkingModeField(
    locale: RwSignal<Locale>,
    thinking_mode_draft: RwSignal<String>,
    hint_class: &'static str,
    select_id: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=select_id>
                {move || i18n::settings_label_llm_thinking_mode(locale.get())}
            </label>
            <select
                id=select_id
                class="settings-select"
                prop:value=move || thinking_mode_draft.get()
                on:change=move |ev| thinking_mode_draft.set(event_target_value(&ev))
            >
                <option value="server">
                    {move || i18n::settings_thinking_mode_server(locale.get())}
                </option>
                <option value="on">
                    {move || i18n::settings_thinking_mode_on(locale.get())}
                </option>
                <option value="off">
                    {move || i18n::settings_thinking_mode_off(locale.get())}
                </option>
            </select>
            <p class=hint_class>{move || i18n::settings_llm_thinking_mode_hint(locale.get())}</p>
        </div>
    }
}

#[component]
pub(crate) fn LlmClientApiKeyField(
    locale: RwSignal<Locale>,
    api_key_draft: RwSignal<String>,
    has_saved_key: RwSignal<bool>,
    clear_key_intent: RwSignal<bool>,
    hint_class: &'static str,
    saved_note_fn: fn(Locale) -> &'static str,
    clear_label_fn: fn(Locale) -> &'static str,
) -> impl IntoView {
    view! {
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
                prop:value=move || api_key_draft.get()
                on:input=move |ev| api_key_draft.set(event_target_value(&ev))
            />
        </div>
        <Show when=move || has_saved_key.get() && !clear_key_intent.get()>
            <p class=hint_class>{move || saved_note_fn(locale.get())}</p>
        </Show>
        <div class="settings-actions-row">
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                prop:disabled=move || !has_saved_key.get() || clear_key_intent.get()
                on:click=move |_| {
                    clear_key_intent.set(true);
                    api_key_draft.set(String::new());
                }
            >
                {move || clear_label_fn(locale.get())}
            </button>
        </div>
    }
}

#[component]
pub(crate) fn LlmExecutorApiKeyField(
    locale: RwSignal<Locale>,
    api_key_draft: RwSignal<String>,
    has_saved_key: RwSignal<bool>,
    clear_key_intent: RwSignal<bool>,
    hint_class: &'static str,
) -> impl IntoView {
    view! {
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
                prop:value=move || api_key_draft.get()
                on:input=move |ev| api_key_draft.set(event_target_value(&ev))
            />
        </div>
        <Show when=move || has_saved_key.get() && !clear_key_intent.get()>
            <p class=hint_class>{move || i18n::settings_executor_key_saved_note(locale.get())}</p>
        </Show>
        <div class="settings-actions-row">
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                prop:disabled=move || !has_saved_key.get() || clear_key_intent.get()
                on:click=move |_| {
                    clear_key_intent.set(true);
                    api_key_draft.set(String::new());
                }
            >
                {move || i18n::settings_clear_executor_key(locale.get())}
            </button>
        </div>
    }
}
