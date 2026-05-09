//! 设置页「模型列表」：新增预设、套用主模型 / 执行器、与 `SavedModelPreset` 持久化配合。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::api::{
    ExecutorLlmDraftSignals, MainLlmDraftSignals, SavedModelPreset,
    apply_saved_model_preset_to_executor_fields, apply_saved_model_preset_to_main_fields,
    saved_model_preset_from_main_drafts,
};
use crate::i18n::{self, Locale};

/// 模型注册表与主/执行器草稿之间的接线（单组件形参，满足 fn-param 棘轮）。
#[derive(Clone, Copy)]
pub(crate) struct SettingsModelsRegistryBundle {
    pub locale: RwSignal<Locale>,
    pub saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    pub main: MainLlmDraftSignals,
    pub main_api_key_draft: RwSignal<String>,
    pub exec: ExecutorLlmDraftSignals,
    pub exec_api_key_draft: RwSignal<String>,
    /// 表单控件 `id` 前缀，避免设置页与弹窗同时挂载时冲突。
    pub form_id_prefix: &'static str,
}

#[derive(Clone, Copy)]
struct RegistryToolbarSignals {
    locale: RwSignal<Locale>,
    add_form_open: RwSignal<bool>,
    form_error: RwSignal<Option<String>>,
}

#[component]
fn SettingsModelsRegistryToolbar(s: RegistryToolbarSignals) -> impl IntoView {
    let RegistryToolbarSignals {
        locale,
        add_form_open,
        form_error,
    } = s;
    view! {
        <div class="settings-model-registry-head">
            <h3 class="settings-block-title">{move || i18n::settings_saved_models_block_title(locale.get())}</h3>
            <button
                type="button"
                class="btn btn-secondary btn-sm settings-model-registry-add"
                prop:aria-expanded=move || add_form_open.get()
                prop:aria-label=move || i18n::settings_models_add_expand_aria(locale.get())
                on:click=move |_| {
                    add_form_open.update(|o| *o = !*o);
                    form_error.set(None);
                }
            >
                "+"
            </button>
        </div>
    }
}

#[derive(Clone, Copy)]
struct RegistrySnapshotSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    main: MainLlmDraftSignals,
    main_api_key_draft: RwSignal<String>,
}

#[component]
fn SettingsModelsRegistrySnapshotBtn(s: RegistrySnapshotSignals) -> impl IntoView {
    let RegistrySnapshotSignals {
        locale,
        saved_model_presets,
        main,
        main_api_key_draft,
    } = s;
    view! {
        <div class="settings-field">
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                on:click=move |_| {
                    let p = saved_model_preset_from_main_drafts(
                        main.llm_api_base_draft.get_untracked().as_str(),
                        main.llm_api_base_preset_select.get_untracked().as_str(),
                        main.llm_model_draft.get_untracked().as_str(),
                        main.llm_temperature_draft.get_untracked().as_str(),
                        main.llm_context_tokens_draft.get_untracked().as_str(),
                        main.llm_thinking_mode_draft.get_untracked().as_str(),
                        main_api_key_draft.get_untracked().as_str(),
                    );
                    saved_model_presets.update(|v| v.push(p));
                }
            >
                {move || i18n::settings_saved_models_add_from_main(locale.get())}
            </button>
        </div>
    }
}

#[derive(Clone)]
struct ManualPresetDraft {
    api_base: String,
    label: String,
    model_id: String,
    api_key: String,
    ctx_tokens: String,
}

fn try_build_manual_saved_preset(d: ManualPresetDraft) -> Result<SavedModelPreset, ()> {
    let base = d.api_base.trim();
    let lab = d.label.trim();
    if base.is_empty() || lab.is_empty() {
        return Err(());
    }
    let mid = d.model_id.trim();
    let model = if mid.is_empty() {
        lab.to_string()
    } else {
        mid.to_string()
    };
    Ok(SavedModelPreset {
        label: lab.to_string(),
        api_base: base.to_string(),
        api_base_preset_select: String::new(),
        model,
        temperature: "0.7".to_string(),
        llm_context_tokens: d.ctx_tokens.trim().to_string(),
        llm_thinking_mode: "server".to_string(),
        api_key: d.api_key,
    })
}

#[derive(Clone)]
struct RegistryAddFormSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    add_form_open: RwSignal<bool>,
    form_error: RwSignal<Option<String>>,
    new_api_base: RwSignal<String>,
    new_label: RwSignal<String>,
    new_model_id: RwSignal<String>,
    new_api_key: RwSignal<String>,
    new_ctx_tokens: RwSignal<String>,
    id_label: String,
    id_base_url: String,
    id_model: String,
    id_key: String,
    id_ctx: String,
}

#[derive(Clone)]
struct RegistryAddFormPrimarySignals {
    locale: RwSignal<Locale>,
    new_label: RwSignal<String>,
    new_api_base: RwSignal<String>,
    new_model_id: RwSignal<String>,
    id_label: String,
    id_base_url: String,
    id_model: String,
}

#[component]
fn SettingsModelsRegistryAddFormPrimaryFields(s: RegistryAddFormPrimarySignals) -> impl IntoView {
    let RegistryAddFormPrimarySignals {
        locale,
        new_label,
        new_api_base,
        new_model_id,
        id_label,
        id_base_url,
        id_model,
    } = s;
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=id_label.clone()>
                {move || i18n::settings_models_label_name(locale.get())}
            </label>
            <input
                type="text"
                class="settings-text-input"
                id=id_label.clone()
                prop:value=move || new_label.get()
                on:input=move |ev| new_label.set(event_target_value(&ev))
            />
        </div>
        <div class="settings-field">
            <label class="settings-field-label" for=id_base_url.clone()>
                {move || i18n::settings_models_label_base_url(locale.get())}
            </label>
            <input
                type="text"
                class="settings-text-input"
                id=id_base_url.clone()
                prop:value=move || new_api_base.get()
                on:input=move |ev| new_api_base.set(event_target_value(&ev))
            />
        </div>
        <div class="settings-field">
            <label class="settings-field-label" for=id_model.clone()>
                {move || i18n::settings_models_label_model_id(locale.get())}
            </label>
            <input
                type="text"
                class="settings-text-input"
                id=id_model.clone()
                prop:value=move || new_model_id.get()
                prop:placeholder=move || i18n::settings_models_ph_model_id(locale.get())
                on:input=move |ev| new_model_id.set(event_target_value(&ev))
            />
        </div>
    }
}

#[derive(Clone)]
struct RegistryAddFormSecondarySignals {
    locale: RwSignal<Locale>,
    new_api_key: RwSignal<String>,
    new_ctx_tokens: RwSignal<String>,
    id_key: String,
    id_ctx: String,
}

#[component]
fn SettingsModelsRegistryAddFormSecondaryFields(
    s: RegistryAddFormSecondarySignals,
) -> impl IntoView {
    let RegistryAddFormSecondarySignals {
        locale,
        new_api_key,
        new_ctx_tokens,
        id_key,
        id_ctx,
    } = s;
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=id_key.clone()>
                {move || i18n::settings_models_label_api_key(locale.get())}
            </label>
            <input
                type="password"
                class="settings-text-input"
                autocomplete="off"
                id=id_key.clone()
                prop:value=move || new_api_key.get()
                on:input=move |ev| new_api_key.set(event_target_value(&ev))
            />
        </div>
        <div class="settings-field">
            <label class="settings-field-label" for=id_ctx.clone()>
                {move || i18n::settings_models_label_context_tokens(locale.get())}
            </label>
            <input
                type="text"
                class="settings-text-input"
                id=id_ctx.clone()
                prop:value=move || new_ctx_tokens.get()
                on:input=move |ev| new_ctx_tokens.set(event_target_value(&ev))
            />
        </div>
    }
}

#[derive(Clone)]
struct RegistryAddFormActionSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    add_form_open: RwSignal<bool>,
    form_error: RwSignal<Option<String>>,
    new_api_base: RwSignal<String>,
    new_label: RwSignal<String>,
    new_model_id: RwSignal<String>,
    new_api_key: RwSignal<String>,
    new_ctx_tokens: RwSignal<String>,
}

#[component]
fn SettingsModelsRegistryAddFormActions(s: RegistryAddFormActionSignals) -> impl IntoView {
    let RegistryAddFormActionSignals {
        locale,
        saved_model_presets,
        add_form_open,
        form_error,
        new_api_base,
        new_label,
        new_model_id,
        new_api_key,
        new_ctx_tokens,
    } = s;
    view! {
        <div class="settings-row">
            <button
                type="button"
                class="btn btn-primary btn-sm"
                on:click=move |_| {
                    let d = ManualPresetDraft {
                        api_base: new_api_base.get_untracked(),
                        label: new_label.get_untracked(),
                        model_id: new_model_id.get_untracked(),
                        api_key: new_api_key.get_untracked(),
                        ctx_tokens: new_ctx_tokens.get_untracked(),
                    };
                    let preset = match try_build_manual_saved_preset(d) {
                        Ok(p) => p,
                        Err(()) => {
                            form_error.set(Some(
                                i18n::settings_models_validation_required(locale.get()).to_string(),
                            ));
                            return;
                        }
                    };
                    form_error.set(None);
                    saved_model_presets.update(|v| v.push(preset));
                    new_api_base.set(String::new());
                    new_label.set(String::new());
                    new_model_id.set(String::new());
                    new_api_key.set(String::new());
                    new_ctx_tokens.set(String::new());
                    add_form_open.set(false);
                }
            >
                {move || i18n::settings_models_add_submit(locale.get())}
            </button>
            <button
                type="button"
                class="btn btn-ghost btn-sm"
                on:click=move |_| {
                    add_form_open.set(false);
                    form_error.set(None);
                }
            >
                {move || i18n::settings_models_cancel_form(locale.get())}
            </button>
        </div>
    }
}

#[component]
fn SettingsModelsRegistryAddForm(s: RegistryAddFormSignals) -> impl IntoView {
    let RegistryAddFormSignals {
        locale,
        saved_model_presets,
        add_form_open,
        form_error,
        new_api_base,
        new_label,
        new_model_id,
        new_api_key,
        new_ctx_tokens,
        id_label,
        id_base_url,
        id_model,
        id_key,
        id_ctx,
    } = s;
    view! {
        <div class="settings-model-registry-form settings-block-nested">
            <Show when=move || form_error.get().is_some()>
                <p class="settings-form-error" role="alert">
                    {move || form_error.get().unwrap_or_default()}
                </p>
            </Show>
            <SettingsModelsRegistryAddFormPrimaryFields s=RegistryAddFormPrimarySignals {
                locale,
                new_label,
                new_api_base,
                new_model_id,
                id_label: id_label.clone(),
                id_base_url: id_base_url.clone(),
                id_model: id_model.clone(),
            } />
            <SettingsModelsRegistryAddFormSecondaryFields s=RegistryAddFormSecondarySignals {
                locale,
                new_api_key,
                new_ctx_tokens,
                id_key: id_key.clone(),
                id_ctx: id_ctx.clone(),
            } />
            <SettingsModelsRegistryAddFormActions s=RegistryAddFormActionSignals {
                locale,
                saved_model_presets,
                add_form_open,
                form_error,
                new_api_base,
                new_label,
                new_model_id,
                new_api_key,
                new_ctx_tokens,
            } />
        </div>
    }
}

#[derive(Clone, Copy)]
struct RegistryPresetRowSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    main: MainLlmDraftSignals,
    main_api_key_draft: RwSignal<String>,
    exec: ExecutorLlmDraftSignals,
    exec_api_key_draft: RwSignal<String>,
}

#[derive(Clone)]
struct RegistryPresetRowModel {
    index: usize,
    preset: SavedModelPreset,
}

#[component]
fn SettingsModelsRegistryPresetRow(
    s: RegistryPresetRowSignals,
    row: RegistryPresetRowModel,
) -> impl IntoView {
    let RegistryPresetRowSignals {
        locale,
        saved_model_presets,
        main,
        main_api_key_draft,
        exec,
        exec_api_key_draft,
    } = s;
    let RegistryPresetRowModel { index, preset } = row;
    let label = preset.label.clone();
    let base_short = preset.api_base.clone();
    let preset_main = preset.clone();
    let preset_exec = preset.clone();
    let trimmed = preset.llm_context_tokens.trim().to_string();
    let ctx_meta = if trimmed.is_empty() {
        None
    } else {
        Some(i18n::settings_models_ctx_line(
            locale.get_untracked(),
            trimmed.as_str(),
        ))
    };
    view! {
        <li class="settings-saved-models-item settings-model-registry-item">
            <div class="settings-model-registry-item-main">
                <span class="settings-saved-models-label">{label}</span>
                <span class="settings-model-registry-meta">{base_short}</span>
                {ctx_meta.map(|line| view! {
                    <span class="settings-model-registry-meta">{line}</span>
                })}
            </div>
            <div class="settings-model-registry-actions">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    on:click=move |_| {
                        apply_saved_model_preset_to_main_fields(&preset_main, main);
                        main_api_key_draft.set(preset_main.api_key.clone());
                    }
                >
                    {move || i18n::settings_models_apply_main_btn(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    on:click=move |_| {
                        apply_saved_model_preset_to_executor_fields(&preset_exec, exec);
                        exec_api_key_draft.set(preset_exec.api_key.clone());
                    }
                >
                    {move || i18n::settings_models_apply_executor_btn(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-ghost btn-sm"
                    on:click=move |_| {
                        saved_model_presets.update(|v| {
                            if index < v.len() {
                                v.remove(index);
                            }
                        });
                    }
                >
                    {move || i18n::settings_saved_models_remove(locale.get())}
                </button>
            </div>
        </li>
    }
}

#[derive(Clone, Copy)]
struct RegistryPresetListSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    main: MainLlmDraftSignals,
    main_api_key_draft: RwSignal<String>,
    exec: ExecutorLlmDraftSignals,
    exec_api_key_draft: RwSignal<String>,
}

#[component]
fn SettingsModelsRegistryPresetList(s: RegistryPresetListSignals) -> impl IntoView {
    let row_sig = RegistryPresetRowSignals {
        locale: s.locale,
        saved_model_presets: s.saved_model_presets,
        main: s.main,
        main_api_key_draft: s.main_api_key_draft,
        exec: s.exec,
        exec_api_key_draft: s.exec_api_key_draft,
    };
    view! {
        <ul class="settings-saved-models-list" role="list">
            <For
                each=move || {
                    s.saved_model_presets
                        .get()
                        .into_iter()
                        .enumerate()
                        .collect::<Vec<(usize, SavedModelPreset)>>()
                }
                key=|(i, p)| format!("saved-model-{i}-{}", p.label)
                children=move |(i, preset)| {
                    view! {
                        <SettingsModelsRegistryPresetRow
                            s=row_sig
                            row=RegistryPresetRowModel { index: i, preset }
                        />
                    }
                }
            />
        </ul>
    }
}

#[component]
pub(crate) fn SettingsModelsRegistryPanel(bundle: SettingsModelsRegistryBundle) -> impl IntoView {
    let SettingsModelsRegistryBundle {
        locale,
        saved_model_presets,
        main,
        main_api_key_draft,
        exec,
        exec_api_key_draft,
        form_id_prefix,
    } = bundle;

    let add_form_open = RwSignal::new(false);
    let form_error = RwSignal::new(Option::<String>::None);
    let new_api_base = RwSignal::new(String::new());
    let new_label = RwSignal::new(String::new());
    let new_model_id = RwSignal::new(String::new());
    let new_api_key = RwSignal::new(String::new());
    let new_ctx_tokens = RwSignal::new(String::new());

    let id_root = format!("{form_id_prefix}-models-new");
    let id_label = format!("{id_root}-label");
    let id_base_url = format!("{id_root}-base");
    let id_model = format!("{id_root}-model");
    let id_key = format!("{id_root}-key");
    let id_ctx = format!("{id_root}-ctx");

    let hint_class = "settings-field-nested-hint";

    view! {
        <div class="settings-block">
            <SettingsModelsRegistryToolbar s=RegistryToolbarSignals {
                locale,
                add_form_open,
                form_error,
            } />
            <p class=hint_class>{move || i18n::settings_saved_models_hint(locale.get())}</p>
            <SettingsModelsRegistrySnapshotBtn s=RegistrySnapshotSignals {
                locale,
                saved_model_presets,
                main,
                main_api_key_draft,
            } />
            <Show when=move || add_form_open.get()>
                <SettingsModelsRegistryAddForm s=RegistryAddFormSignals {
                    locale,
                    saved_model_presets,
                    add_form_open,
                    form_error,
                    new_api_base,
                    new_label,
                    new_model_id,
                    new_api_key,
                    new_ctx_tokens,
                    id_label: id_label.clone(),
                    id_base_url: id_base_url.clone(),
                    id_model: id_model.clone(),
                    id_key: id_key.clone(),
                    id_ctx: id_ctx.clone(),
                } />
            </Show>
            <SettingsModelsRegistryPresetList s=RegistryPresetListSignals {
                locale,
                saved_model_presets,
                main,
                main_api_key_draft,
                exec,
                exec_api_key_draft,
            } />
        </div>
    }
}
