use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use std::vec::Vec;

use crate::api::{
    ExecutorLlmDraftSignals, MainLlmDraftSignals, SavedModelPreset,
    apply_saved_model_preset_to_executor_fields, apply_saved_model_preset_to_main_fields,
    saved_model_preset_from_main_drafts,
};
use crate::app_prefs::THEME_SLUGS;
use crate::i18n::{self, Locale};
use crate::settings_llm_fields::{
    LlmApiBasePresetSelect, LlmClientApiKeyField, LlmContextTokensField, LlmCustomApiBaseInput,
    LlmExecutorApiKeyField, LlmModelField, LlmTemperatureField, LlmThinkingModeField,
    OptionalLlmExecutionModeField,
};

/// 设置页「主 LLM」区块所需信号（缩短 [`SettingsLlmBlock`] 形参列表；勿命名为 `*Props`，与 Leptos 组件宏生成类型冲突）。
#[derive(Clone)]
pub(crate) struct SettingsLlmBlockBundle {
    pub locale: RwSignal<Locale>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub execution_mode_draft: Option<RwSignal<String>>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub clear_client_key_intent: RwSignal<bool>,
    pub hint_class: &'static str,
    /// `<select id=…>`：设置页与弹窗可能同时挂载，须用不同 id。
    pub llm_thinking_mode_select_id: &'static str,
}

#[component]
pub(crate) fn SettingsAppearanceBlock(
    locale: RwSignal<Locale>,
    appearance_locale: RwSignal<Locale>,
    appearance_theme: RwSignal<String>,
    appearance_bg_decor: RwSignal<bool>,
    /// `<select id=…>`：设置页与弹窗可能同时挂载，须用不同 id。
    theme_select_id: &'static str,
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
            <div class="settings-field">
                <label class="settings-field-label" for=theme_select_id>
                    {move || i18n::settings_label_theme_preset(locale.get())}
                </label>
                <select
                    id=theme_select_id
                    class="settings-select"
                    prop:value=move || appearance_theme.get()
                    on:change=move |ev| appearance_theme.set(event_target_value(&ev))
                >
                    {THEME_SLUGS.iter().copied().map(|slug| {
                        view! {
                            <option value=slug>
                                {move || i18n::settings_theme_preset_label(locale.get(), slug)}
                            </option>
                        }
                    }).collect_view()}
                </select>
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
pub(crate) fn SettingsLlmBlock(bundle: SettingsLlmBlockBundle) -> impl IntoView {
    let SettingsLlmBlockBundle {
        locale,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
        execution_mode_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        clear_client_key_intent,
        hint_class,
        llm_thinking_mode_select_id,
    } = bundle;
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_llm(locale.get())}</h3>
            <p class=hint_class>{move || i18n::settings_llm_hint(locale.get())}</p>
            <LlmApiBasePresetSelect
                locale
                api_base_draft=llm_api_base_draft
                api_base_preset_select=llm_api_base_preset_select
                model_draft=llm_model_draft
                select_id="settings-llm-api-base-preset"
            />
            <LlmCustomApiBaseInput
                locale
                api_base_draft=llm_api_base_draft
                api_base_preset_select=llm_api_base_preset_select
                label_fn=i18n::settings_label_api_base
                placeholder_fn=i18n::settings_ph_api_base
                input_id="settings-llm-api-base"
            />
            <LlmModelField
                locale
                model_draft=llm_model_draft
                label_fn=i18n::settings_label_model
                placeholder_fn=i18n::settings_ph_model
                input_id="settings-llm-model"
            />
            <LlmTemperatureField locale temperature_draft=llm_temperature_draft hint_class />
            <LlmContextTokensField locale llm_context_tokens_draft hint_class />
            <LlmThinkingModeField
                locale
                thinking_mode_draft=llm_thinking_mode_draft
                hint_class
                select_id=llm_thinking_mode_select_id
            />
            <OptionalLlmExecutionModeField locale execution_mode_draft hint_class />
            <LlmClientApiKeyField
                locale
                api_key_draft=llm_api_key_draft
                has_saved_key=llm_has_saved_key
                clear_key_intent=clear_client_key_intent
                hint_class
                saved_note_fn=i18n::settings_key_saved_note
                clear_label_fn=i18n::settings_clear_key
            />
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
            <LlmApiBasePresetSelect
                locale
                api_base_draft=executor_llm_api_base_draft
                api_base_preset_select=executor_llm_api_base_preset_select
                model_draft=executor_llm_model_draft
                select_id="settings-executor-llm-api-base-preset"
            />
            <LlmCustomApiBaseInput
                locale
                api_base_draft=executor_llm_api_base_draft
                api_base_preset_select=executor_llm_api_base_preset_select
                label_fn=i18n::settings_label_executor_api_base
                placeholder_fn=i18n::settings_ph_api_base
                input_id="settings-executor-llm-api-base"
            />
            <LlmModelField
                locale
                model_draft=executor_llm_model_draft
                label_fn=i18n::settings_label_executor_model
                placeholder_fn=i18n::settings_ph_model
                input_id="settings-executor-llm-model"
            />
            <LlmExecutorApiKeyField
                locale
                api_key_draft=executor_llm_api_key_draft
                has_saved_key=executor_llm_has_saved_key
                clear_key_intent=clear_executor_key_intent
                hint_class
            />
        </div>
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SettingsSavedModelsPresetsBundle {
    pub locale: RwSignal<Locale>,
    pub saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    /// `<select id=…>`：设置页与弹窗分区须区分。
    pub main_select_id: &'static str,
    pub main: MainLlmDraftSignals,
}

fn preset_list_index_from_select_value(raw: &str) -> Option<usize> {
    if raw.is_empty() {
        return None;
    }
    raw.parse().ok()
}

#[component]
pub(crate) fn SettingsSavedModelsPresetsPanel(
    bundle: SettingsSavedModelsPresetsBundle,
) -> impl IntoView {
    let SettingsSavedModelsPresetsBundle {
        locale,
        saved_model_presets,
        main_select_id,
        main,
    } = bundle;
    let hint_class = "settings-field-nested-hint";
    let pick_main = RwSignal::new(String::new());

    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_saved_models_block_title(locale.get())}</h3>
            <p class=hint_class>{move || i18n::settings_saved_models_hint(locale.get())}</p>
            <div class="settings-field">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    on:click=move |_| {
                        let p = saved_model_preset_from_main_drafts(
                            main.llm_api_base_draft.get().as_str(),
                            main.llm_api_base_preset_select.get().as_str(),
                            main.llm_model_draft.get().as_str(),
                            main.llm_temperature_draft.get().as_str(),
                            main.llm_context_tokens_draft.get().as_str(),
                            main.llm_thinking_mode_draft.get().as_str(),
                        );
                        saved_model_presets.update(|v| v.push(p));
                    }
                >
                    {move || i18n::settings_saved_models_add_from_main(locale.get())}
                </button>
            </div>
            <ul class="settings-saved-models-list" role="list">
                <For
                    each=move || {
                        saved_model_presets
                            .get()
                            .into_iter()
                            .enumerate()
                            .collect::<Vec<(usize, SavedModelPreset)>>()
                    }
                    key=|(i, p)| format!("saved-model-{i}-{}", p.label)
                    children=move |(i, preset)| {
                        let label = preset.label.clone();
                        view! {
                            <li class="settings-saved-models-item">
                                <span class="settings-saved-models-label">{label}</span>
                                <button
                                    type="button"
                                    class="btn btn-ghost btn-sm"
                                    on:click=move |_| {
                                        saved_model_presets.update(|v| {
                                            if i < v.len() {
                                                v.remove(i);
                                            }
                                        });
                                    }
                                >
                                    {move || i18n::settings_saved_models_remove(locale.get())}
                                </button>
                            </li>
                        }
                    }
                />
            </ul>
            <div class="settings-field">
                <label class="settings-field-label" for=main_select_id>
                    {move || i18n::settings_saved_models_select_main(locale.get())}
                </label>
                <select
                    id=main_select_id
                    class="settings-select"
                    prop:value=move || pick_main.get()
                    on:change=move |ev| {
                        let raw = event_target_value(&ev);
                        pick_main.set(String::new());
                        let Some(idx) = preset_list_index_from_select_value(raw.as_str()) else {
                            return;
                        };
                        let presets = saved_model_presets.get();
                        let Some(p) = presets.get(idx) else {
                            return;
                        };
                        apply_saved_model_preset_to_main_fields(p, main);
                    }
                >
                    <option value="">{move || i18n::settings_saved_models_pick_placeholder(locale.get())}</option>
                    {move || {
                        saved_model_presets
                            .get()
                            .into_iter()
                            .enumerate()
                            .map(|(i, p)| {
                                let v = i.to_string();
                                let lab = p.label.clone();
                                view! {
                                    <option value=v>{lab}</option>
                                }
                            })
                            .collect_view()
                    }}
                </select>
            </div>
        </div>
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SettingsExecutorSavedPresetBundle {
    pub locale: RwSignal<Locale>,
    pub saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    pub select_id: &'static str,
    pub exec: ExecutorLlmDraftSignals,
}

#[component]
pub(crate) fn SettingsExecutorSavedPresetPicker(
    bundle: SettingsExecutorSavedPresetBundle,
) -> impl IntoView {
    let SettingsExecutorSavedPresetBundle {
        locale,
        saved_model_presets,
        select_id,
        exec,
    } = bundle;
    let pick_ex = RwSignal::new(String::new());

    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=select_id>
                {move || i18n::settings_saved_models_select_executor(locale.get())}
            </label>
            <select
                id=select_id
                class="settings-select"
                prop:value=move || pick_ex.get()
                on:change=move |ev| {
                    let raw = event_target_value(&ev);
                    pick_ex.set(String::new());
                    let Some(idx) = preset_list_index_from_select_value(raw.as_str()) else {
                        return;
                    };
                    let presets = saved_model_presets.get();
                    let Some(p) = presets.get(idx) else {
                        return;
                    };
                    apply_saved_model_preset_to_executor_fields(p, exec);
                }
            >
                <option value="">{move || i18n::settings_saved_models_pick_placeholder(locale.get())}</option>
                {move || {
                    saved_model_presets
                        .get()
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let v = i.to_string();
                            let lab = p.label.clone();
                            view! {
                                <option value=v>{lab}</option>
                            }
                        })
                        .collect_view()
                }}
            </select>
        </div>
    }
}

#[component]
pub(crate) fn SettingsToolsBlock(
    locale: RwSignal<Locale>,
    readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_tools_readonly_ttl_block_title(locale.get())}</h3>
            <p class="settings-field-nested-hint">{move || i18n::settings_tools_readonly_ttl_cache_hint(locale.get())}</p>
            <label class="settings-checkbox-label">
                <input
                    type="checkbox"
                    prop:checked=move || readonly_tool_ttl_cache_follow_server.get()
                    on:change=move |_| {
                        readonly_tool_ttl_cache_follow_server.update(|v| *v = !*v);
                    }
                />
                <span>{move || i18n::settings_tools_readonly_ttl_cache_label(locale.get())}</span>
            </label>
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
