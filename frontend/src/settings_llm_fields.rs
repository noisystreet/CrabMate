//! 设置页 LLM 表单字段子组件（降低 `settings_sections` 中单组件圈复杂度）。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::api::{
    ExecutorLlmDraftSignals, MainLlmDraftSignals, apply_saved_model_preset_to_executor_fields,
    apply_saved_model_preset_to_main_fields, matching_saved_preset_index,
};
use crate::i18n::{self, Locale};

#[component]
pub(crate) fn LlmContextTokensField(
    locale: RwSignal<Locale>,
    llm_context_tokens_draft: RwSignal<String>,
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
        </div>
    }
}

#[component]
pub(crate) fn LlmTemperatureField(
    locale: RwSignal<Locale>,
    temperature_draft: RwSignal<String>,
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
        </div>
    }
}

#[component]
pub(crate) fn LlmExecutionModeField(
    locale: RwSignal<Locale>,
    execution_mode_draft: RwSignal<String>,
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
        </div>
    }
}

#[component]
pub(crate) fn OptionalLlmExecutionModeField(
    locale: RwSignal<Locale>,
    execution_mode_draft: Option<RwSignal<String>>,
) -> impl IntoView {
    match execution_mode_draft {
        Some(sig) => view! { <LlmExecutionModeField locale execution_mode_draft=sig /> }.into_any(),
        None => view! { <></> }.into_any(),
    }
}

#[component]
pub(crate) fn LlmThinkingModeField(
    locale: RwSignal<Locale>,
    thinking_mode_draft: RwSignal<String>,
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
        </div>
    }
}

#[derive(Clone, Copy)]
pub(crate) enum LlmSavedPresetApplyTarget {
    Main(
        MainLlmDraftSignals,
        RwSignal<String>,
        RwSignal<bool>,
        RwSignal<bool>,
    ),
    Executor(
        ExecutorLlmDraftSignals,
        RwSignal<String>,
        RwSignal<bool>,
        RwSignal<bool>,
    ),
}

fn apply_saved_pick(target: LlmSavedPresetApplyTarget, preset: &crate::api::SavedModelPreset) {
    if !preset.enabled {
        return;
    }
    match target {
        LlmSavedPresetApplyTarget::Main(drafts, api_key, has_saved_key, clear_key_intent) => {
            apply_saved_model_preset_to_main_fields(preset, drafts);
            api_key.set(preset.api_key.clone());
            has_saved_key.set(!preset.api_key.trim().is_empty());
            clear_key_intent.set(false);
        }
        LlmSavedPresetApplyTarget::Executor(drafts, api_key, has_saved_key, clear_key_intent) => {
            apply_saved_model_preset_to_executor_fields(preset, drafts);
            api_key.set(preset.api_key.clone());
            has_saved_key.set(!preset.api_key.trim().is_empty());
            clear_key_intent.set(false);
        }
    }
}

fn saved_preset_select_value(
    target: LlmSavedPresetApplyTarget,
    presets: &[crate::api::SavedModelPreset],
) -> String {
    let idx = match target {
        LlmSavedPresetApplyTarget::Main(d, ..) => matching_saved_preset_index(
            presets,
            d.llm_api_base_draft.get_untracked().as_str(),
            d.llm_api_base_preset_select.get_untracked().as_str(),
            d.llm_model_draft.get_untracked().as_str(),
        ),
        LlmSavedPresetApplyTarget::Executor(d, ..) => matching_saved_preset_index(
            presets,
            d.executor_llm_api_base_draft.get_untracked().as_str(),
            d.executor_llm_api_base_preset_select
                .get_untracked()
                .as_str(),
            d.executor_llm_model_draft.get_untracked().as_str(),
        ),
    };
    idx.map(|i| i.to_string()).unwrap_or_default()
}

/// 从「已保存模型」列表选择一条，写入主模型或执行器草稿（含 API Key 与 `has_saved_key`）。
#[component]
pub(crate) fn LlmSavedPresetPicker(
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
    pick_target: LlmSavedPresetApplyTarget,
    select_id: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=select_id>
                {move || i18n::settings_label_saved_model_pick(locale.get())}
            </label>
            <select
                id=select_id
                class="settings-select"
                prop:value=move || {
                    let presets = saved_model_presets.get();
                    saved_preset_select_value(pick_target, presets.as_slice())
                }
                prop:disabled=move || {
                    let v = saved_model_presets.get();
                    v.is_empty() || v.iter().all(|p| !p.enabled)
                }
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    if v.trim().is_empty() {
                        return;
                    }
                    let Ok(i) = v.parse::<usize>() else {
                        return;
                    };
                    let Some(preset) = saved_model_presets
                        .with_untracked(|list| list.get(i).cloned())
                    else {
                        return;
                    };
                    if !preset.enabled {
                        return;
                    }
                    apply_saved_pick(pick_target, &preset);
                }
            >
                <option value="">
                    {move || i18n::settings_saved_model_pick_placeholder(locale.get())}
                </option>
                {move || {
                    let loc = locale.get();
                    saved_model_presets
                        .get()
                        .into_iter()
                        .enumerate()
                        .map(|(i, preset)| {
                            let val = i.to_string();
                            let mut lab = preset.label.clone();
                            if !preset.enabled {
                                lab.push_str(i18n::settings_models_preset_disabled_suffix(loc));
                            }
                            let disabled = !preset.enabled;
                            view! { <option value=val disabled=disabled>{lab}</option> }
                        })
                        .collect_view()
                }}
            </select>
        </div>
    }
}
