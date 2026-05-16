//! 设置页「模型列表」：新增/编辑预设、启用开关、持久化 `SavedModelPreset`（`persist` 子模块见同目录）。

mod delete_confirm;
mod persist;

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use std::sync::Arc;

use crate::a11y::{focus_first_in_modal_container, trap_tab_in_container};

use crate::api::SavedModelPreset;
use crate::i18n::{self, Locale};
use persist::try_persist_saved_presets_with_feedback;

/// 模型注册表接线（单组件形参，满足 fn-param 棘轮）。
#[derive(Clone)]
pub(crate) struct SettingsModelsRegistryBundle {
    pub locale: RwSignal<Locale>,
    pub saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    /// 表单控件 `id` 前缀，避免设置页与弹窗同时挂载时冲突。
    pub form_id_prefix: &'static str,
    /// 行内开关写入 `localStorage` 后刷新设置 dirty baseline，避免与「放弃更改」不一致。
    pub sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    /// 与设置页/弹窗顶栏反馈共用（如本机列表持久化失败）。
    pub llm_settings_feedback: RwSignal<Option<String>>,
}

/// 「+」添加与行内编辑共用同一弹窗。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum RegistryPresetDialogKind {
    Add,
    Edit(usize),
}

/// 模型注册表 UI 顶层阶段（弹窗与行内删除确认互斥语义集中于此，供 `data-*` / 调试挂钩）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RegistrySurfacePhase {
    Idle,
    DialogOpen,
    PendingDelete,
}

#[inline]
pub(crate) fn derive_registry_surface_phase(
    dialog_mode: Option<RegistryPresetDialogKind>,
    pending_delete_row_key: Option<&str>,
) -> RegistrySurfacePhase {
    if pending_delete_row_key.is_some_and(|k| !k.trim().is_empty()) {
        RegistrySurfacePhase::PendingDelete
    } else if dialog_mode.is_some() {
        RegistrySurfacePhase::DialogOpen
    } else {
        RegistrySurfacePhase::Idle
    }
}

#[inline]
pub(crate) fn registry_surface_phase_data_attr(phase: RegistrySurfacePhase) -> &'static str {
    match phase {
        RegistrySurfacePhase::Idle => "idle",
        RegistrySurfacePhase::DialogOpen => "dialog",
        RegistrySurfacePhase::PendingDelete => "pending-delete",
    }
}

#[derive(Clone)]
struct RegistryToolbarSignals {
    locale: RwSignal<Locale>,
    dialog_mode: RwSignal<Option<RegistryPresetDialogKind>>,
    form_error: RwSignal<Option<String>>,
    clear_form_for_add: Arc<dyn Fn() + Send + Sync>,
    pending_delete_row_key: RwSignal<Option<String>>,
}

#[component]
fn SettingsModelsRegistryToolbar(s: RegistryToolbarSignals) -> impl IntoView {
    let RegistryToolbarSignals {
        locale,
        dialog_mode,
        form_error,
        clear_form_for_add,
        pending_delete_row_key,
    } = s;
    view! {
        <div class="settings-model-registry-head">
            <h3 class="settings-block-title">{move || i18n::settings_saved_models_block_title(locale.get())}</h3>
            <button
                type="button"
                class="btn btn-secondary btn-sm settings-model-registry-add"
                prop:aria-label=move || i18n::settings_models_add_open_aria(locale.get())
                prop:title=move || i18n::settings_models_add_open_aria(locale.get())
                on:click=move |_| {
                    clear_form_for_add();
                    pending_delete_row_key.set(None);
                    dialog_mode.set(Some(RegistryPresetDialogKind::Add));
                    form_error.set(None);
                }
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
                    <path d="M12 5v14M5 12h14" />
                </svg>
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
    temperature: String,
    thinking_mode: String,
}

fn try_build_manual_saved_preset(
    d: &ManualPresetDraft,
    enabled: bool,
) -> Result<SavedModelPreset, ()> {
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
    let temp = d.temperature.trim();
    let temperature = if temp.is_empty() {
        "0.7".to_string()
    } else {
        temp.to_string()
    };
    let think = d.thinking_mode.trim();
    let llm_thinking_mode = match think {
        "on" => "on".to_string(),
        "off" => "off".to_string(),
        _ => "server".to_string(),
    };
    Ok(SavedModelPreset {
        label: lab.to_string(),
        api_base: base.to_string(),
        api_base_preset_select: String::new(),
        model,
        temperature,
        llm_context_tokens: d.ctx_tokens.trim().to_string(),
        llm_thinking_mode,
        api_key: d.api_key.clone(),
        enabled,
    })
}

#[derive(Clone)]
struct RegistryAddFormSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    dialog_mode: RwSignal<Option<RegistryPresetDialogKind>>,
    form_error: RwSignal<Option<String>>,
    new_api_base: RwSignal<String>,
    new_label: RwSignal<String>,
    new_model_id: RwSignal<String>,
    new_api_key: RwSignal<String>,
    new_ctx_tokens: RwSignal<String>,
    new_temperature: RwSignal<String>,
    new_thinking_mode: RwSignal<String>,
    id_label: String,
    id_base_url: String,
    id_model: String,
    id_key: String,
    id_ctx: String,
    id_temp: String,
    id_thinking: String,
    /// `<h2 id=…>` 与 `aria-labelledby`，须与 `form_id_prefix` 组合保证唯一。
    dialog_title_id: String,
    sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    llm_settings_feedback: RwSignal<Option<String>>,
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
struct RegistryAddFormExtraSignals {
    locale: RwSignal<Locale>,
    new_temperature: RwSignal<String>,
    new_thinking_mode: RwSignal<String>,
    id_temp: String,
    id_thinking: String,
}

#[component]
fn SettingsModelsRegistryPresetFormExtraFields(s: RegistryAddFormExtraSignals) -> impl IntoView {
    let RegistryAddFormExtraSignals {
        locale,
        new_temperature,
        new_thinking_mode,
        id_temp,
        id_thinking,
    } = s;
    view! {
        <div class="settings-field">
            <label class="settings-field-label" for=id_temp.clone()>
                {move || i18n::settings_label_temperature(locale.get())}
            </label>
            <input
                type="number"
                class="settings-text-input"
                id=id_temp.clone()
                min="0"
                max="2"
                step="any"
                inputmode="decimal"
                prop:placeholder=move || i18n::settings_ph_temperature(locale.get())
                prop:value=move || new_temperature.get()
                on:input=move |ev| new_temperature.set(event_target_value(&ev))
            />
        </div>
        <div class="settings-field">
            <label class="settings-field-label" for=id_thinking.clone()>
                {move || i18n::settings_label_llm_thinking_mode(locale.get())}
            </label>
            <select
                id=id_thinking.clone()
                class="settings-select"
                prop:value=move || new_thinking_mode.get()
                on:change=move |ev| new_thinking_mode.set(event_target_value(&ev))
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

#[derive(Clone)]
struct RegistryAddFormActionSignals {
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    dialog_mode: RwSignal<Option<RegistryPresetDialogKind>>,
    form_error: RwSignal<Option<String>>,
    new_api_base: RwSignal<String>,
    new_label: RwSignal<String>,
    new_model_id: RwSignal<String>,
    new_api_key: RwSignal<String>,
    new_ctx_tokens: RwSignal<String>,
    new_temperature: RwSignal<String>,
    new_thinking_mode: RwSignal<String>,
    sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    llm_settings_feedback: RwSignal<Option<String>>,
}

#[component]
fn SettingsModelsRegistryAddFormActions(s: RegistryAddFormActionSignals) -> impl IntoView {
    let RegistryAddFormActionSignals {
        locale,
        saved_model_presets,
        dialog_mode,
        form_error,
        new_api_base,
        new_label,
        new_model_id,
        new_api_key,
        new_ctx_tokens,
        new_temperature,
        new_thinking_mode,
        sync_saved_presets_baseline,
        llm_settings_feedback,
    } = s;
    let close_dialog = move || {
        dialog_mode.set(None);
        form_error.set(None);
    };
    let reset_fields = move || {
        new_api_base.set(String::new());
        new_label.set(String::new());
        new_model_id.set(String::new());
        new_api_key.set(String::new());
        new_ctx_tokens.set(String::new());
        new_temperature.set("0.7".to_string());
        new_thinking_mode.set("server".to_string());
    };
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
                        temperature: new_temperature.get_untracked(),
                        thinking_mode: new_thinking_mode.get_untracked(),
                    };
                    let mode = dialog_mode.get_untracked();
                    let enabled_for_preset = match mode {
                        Some(RegistryPresetDialogKind::Add) => true,
                        Some(RegistryPresetDialogKind::Edit(i)) => saved_model_presets
                            .with_untracked(|v| v.get(i).map(|p| p.enabled).unwrap_or(true)),
                        None => true,
                    };
                    let preset = match try_build_manual_saved_preset(&d, enabled_for_preset) {
                        Ok(p) => p,
                        Err(()) => {
                            form_error.set(Some(
                                i18n::settings_models_validation_required(locale.get()).to_string(),
                            ));
                            return;
                        }
                    };
                    form_error.set(None);
                    let loc = locale.get_untracked();
                    let mut next = saved_model_presets.with_untracked(|v| v.clone());
                    match mode {
                        Some(RegistryPresetDialogKind::Add) => next.push(preset),
                        Some(RegistryPresetDialogKind::Edit(i)) if i < next.len() => {
                            next[i] = preset;
                        }
                        Some(RegistryPresetDialogKind::Edit(_)) => return,
                        None => return,
                    }
                    if try_persist_saved_presets_with_feedback(
                        next,
                        loc,
                        saved_model_presets,
                        &sync_saved_presets_baseline,
                        llm_settings_feedback,
                    ) {
                        reset_fields();
                        close_dialog();
                    }
                }
            >
                {move || match dialog_mode.get() {
                    Some(RegistryPresetDialogKind::Edit(_)) => {
                        i18n::settings_models_edit_submit(locale.get())
                    }
                    _ => i18n::settings_models_add_submit(locale.get()),
                }}
            </button>
            <button
                type="button"
                class="btn btn-ghost btn-sm"
                on:click=move |_| {
                    reset_fields();
                    close_dialog();
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
        dialog_mode,
        form_error,
        new_api_base,
        new_label,
        new_model_id,
        new_api_key,
        new_ctx_tokens,
        new_temperature,
        new_thinking_mode,
        id_label,
        id_base_url,
        id_model,
        id_key,
        id_ctx,
        id_temp,
        id_thinking,
        sync_saved_presets_baseline,
        llm_settings_feedback,
        ..
    } = s;
    view! {
        <div class="settings-model-registry-dialog-form">
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
            <SettingsModelsRegistryPresetFormExtraFields s=RegistryAddFormExtraSignals {
                locale,
                new_temperature,
                new_thinking_mode,
                id_temp: id_temp.clone(),
                id_thinking: id_thinking.clone(),
            } />
            <SettingsModelsRegistryAddFormActions s=RegistryAddFormActionSignals {
                locale,
                saved_model_presets,
                dialog_mode,
                form_error,
                new_api_base,
                new_label,
                new_model_id,
                new_api_key,
                new_ctx_tokens,
                new_temperature,
                new_thinking_mode,
                sync_saved_presets_baseline: sync_saved_presets_baseline.clone(),
                llm_settings_feedback,
            } />
        </div>
    }
}

#[component]
fn SettingsModelsRegistryAddModelDialog(s: RegistryAddFormSignals) -> impl IntoView {
    let RegistryAddFormSignals {
        locale,
        dialog_mode,
        form_error,
        dialog_title_id,
        new_api_base,
        new_label,
        new_model_id,
        new_api_key,
        new_ctx_tokens,
        new_temperature,
        new_thinking_mode,
        ..
    } = s.clone();
    let dialog_ref = NodeRef::<Div>::new();
    let title_id_for_aria = dialog_title_id.clone();
    let title_id_for_heading = dialog_title_id.clone();
    let reset_fields = move || {
        new_api_base.set(String::new());
        new_label.set(String::new());
        new_model_id.set(String::new());
        new_api_key.set(String::new());
        new_ctx_tokens.set(String::new());
        new_temperature.set("0.7".to_string());
        new_thinking_mode.set("server".to_string());
    };
    let close_dialog = move || {
        dialog_mode.set(None);
        form_error.set(None);
    };

    Effect::new({
        let dialog_ref = dialog_ref.clone();
        let dialog_mode = dialog_mode;
        move |_| {
            if dialog_mode.get().is_none() {
                return;
            }
            let r = dialog_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = r.get() {
                    focus_first_in_modal_container(el.as_ref());
                }
            });
        }
    });

    view! {
        <Show when=move || dialog_mode.get().is_some()>
            <div
                class="modal-backdrop settings-model-add-dialog-backdrop"
                on:click=move |_| {
                    reset_fields();
                    close_dialog();
                }
            >
                <div
                    class="modal settings-model-add-dialog"
                    node_ref=dialog_ref
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby=title_id_for_aria.clone()
                    tabindex="-1"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Tab" {
                            if let Some(el) = dialog_ref.get() {
                                trap_tab_in_container(&ev, el.as_ref());
                            }
                            return;
                        }
                        if ev.key() == "Escape" {
                            ev.prevent_default();
                            ev.stop_propagation();
                            reset_fields();
                            close_dialog();
                        }
                    }
                >
                    <div class="modal-head">
                        <h2 class="modal-title" id=title_id_for_heading.clone()>
                            {move || match dialog_mode.get() {
                                Some(RegistryPresetDialogKind::Edit(_)) => {
                                    i18n::settings_models_edit_dialog_title(locale.get())
                                }
                                _ => i18n::settings_models_add_dialog_title(locale.get()),
                            }}
                        </h2>
                        <span class="modal-head-spacer"></span>
                        <button
                            type="button"
                            class="btn btn-ghost btn-sm"
                            on:click=move |_| {
                                reset_fields();
                                close_dialog();
                            }
                        >
                            {move || i18n::settings_close(locale.get())}
                        </button>
                    </div>
                    <div class="modal-body">
                        <SettingsModelsRegistryAddForm s=s.clone() />
                    </div>
                </div>
            </div>
        </Show>
    }
}

mod preset_list;

use preset_list::{RegistryPresetListSignals, SettingsModelsRegistryPresetList};

#[component]
pub(crate) fn SettingsModelsRegistryPanel(bundle: SettingsModelsRegistryBundle) -> impl IntoView {
    let SettingsModelsRegistryBundle {
        locale,
        saved_model_presets,
        form_id_prefix,
        sync_saved_presets_baseline,
        llm_settings_feedback,
    } = bundle;

    let dialog_mode = RwSignal::new(None::<RegistryPresetDialogKind>);
    let form_error = RwSignal::new(Option::<String>::None);
    let pending_delete_row_key = RwSignal::new(Option::<String>::None);
    let new_api_base = RwSignal::new(String::new());
    let new_label = RwSignal::new(String::new());
    let new_model_id = RwSignal::new(String::new());
    let new_api_key = RwSignal::new(String::new());
    let new_ctx_tokens = RwSignal::new(String::new());
    let new_temperature = RwSignal::new("0.7".to_string());
    let new_thinking_mode = RwSignal::new("server".to_string());

    let id_root = format!("{form_id_prefix}-models-new");
    let id_label = format!("{id_root}-label");
    let id_base_url = format!("{id_root}-base");
    let id_model = format!("{id_root}-model");
    let id_key = format!("{id_root}-key");
    let id_ctx = format!("{id_root}-ctx");
    let id_temp = format!("{id_root}-temp");
    let id_thinking = format!("{id_root}-thinking");
    let dialog_title_id = format!("{form_id_prefix}-add-model-dialog-title");

    let clear_form_for_add = {
        let new_api_base = new_api_base;
        let new_label = new_label;
        let new_model_id = new_model_id;
        let new_api_key = new_api_key;
        let new_ctx_tokens = new_ctx_tokens;
        let new_temperature = new_temperature;
        let new_thinking_mode = new_thinking_mode;
        Arc::new(move || {
            new_api_base.set(String::new());
            new_label.set(String::new());
            new_model_id.set(String::new());
            new_api_key.set(String::new());
            new_ctx_tokens.set(String::new());
            new_temperature.set("0.7".to_string());
            new_thinking_mode.set("server".to_string());
        }) as Arc<dyn Fn() + Send + Sync>
    };

    let registry_surface = Memo::new(move |_| {
        derive_registry_surface_phase(dialog_mode.get(), pending_delete_row_key.get().as_deref())
    });

    view! {
        <div
            class="settings-block"
            prop:data-crabmate-registry-surface=move || {
                registry_surface_phase_data_attr(registry_surface.get())
            }
        >
            <SettingsModelsRegistryToolbar s=RegistryToolbarSignals {
                locale,
                dialog_mode,
                form_error,
                clear_form_for_add: clear_form_for_add.clone(),
                pending_delete_row_key,
            } />
            <SettingsModelsRegistryAddModelDialog s=RegistryAddFormSignals {
                locale,
                saved_model_presets,
                dialog_mode,
                form_error,
                new_api_base,
                new_label,
                new_model_id,
                new_api_key,
                new_ctx_tokens,
                new_temperature,
                new_thinking_mode,
                id_label: id_label.clone(),
                id_base_url: id_base_url.clone(),
                id_model: id_model.clone(),
                id_key: id_key.clone(),
                id_ctx: id_ctx.clone(),
                id_temp: id_temp.clone(),
                id_thinking: id_thinking.clone(),
                dialog_title_id,
                sync_saved_presets_baseline: sync_saved_presets_baseline.clone(),
                llm_settings_feedback,
            } />
            <SettingsModelsRegistryPresetList s=RegistryPresetListSignals {
                locale,
                saved_model_presets,
                dialog_mode,
                form_error,
                new_api_base,
                new_label,
                new_model_id,
                new_api_key,
                new_ctx_tokens,
                new_temperature,
                new_thinking_mode,
                sync_saved_presets_baseline,
                llm_settings_feedback,
                pending_delete_row_key,
            } />
        </div>
    }
}

#[cfg(test)]
mod registry_surface_phase_tests {
    use super::*;

    #[test]
    fn pending_delete_wins_over_dialog_open() {
        assert_eq!(
            derive_registry_surface_phase(Some(RegistryPresetDialogKind::Add), Some("row-key"),),
            RegistrySurfacePhase::PendingDelete
        );
    }

    #[test]
    fn dialog_without_pending_maps_open() {
        assert_eq!(
            derive_registry_surface_phase(Some(RegistryPresetDialogKind::Edit(0)), None),
            RegistrySurfacePhase::DialogOpen
        );
    }

    #[test]
    fn empty_pending_key_ignored_for_phase() {
        assert_eq!(
            derive_registry_surface_phase(None, Some("   ")),
            RegistrySurfacePhase::Idle
        );
    }
}
