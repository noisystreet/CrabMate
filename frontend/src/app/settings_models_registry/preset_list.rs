use std::sync::Arc;

use leptos::prelude::*;

use crate::api::SavedModelPreset;
use crate::i18n::{self, Locale};

use super::RegistryPresetDialogKind;
use super::delete_confirm::SettingsModelsDeleteConfirm;
use super::persist::try_persist_saved_presets_with_feedback;

/// `<ForEnumerate>` 的 `key`：须稳定且尽量唯一（完全相同的重复项仍可能冲突）。
pub(super) fn saved_preset_row_key(p: &SavedModelPreset) -> String {
    format!(
        "{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}\x1f{}",
        p.label,
        p.api_base,
        p.api_base_preset_select,
        p.model,
        p.temperature,
        p.llm_context_tokens,
        p.llm_thinking_mode,
        p.api_key,
        p.enabled
    )
}

#[derive(Clone)]
struct RegistryPresetRowSignals {
    pub(crate) locale: RwSignal<Locale>,
    pub(crate) saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    pub(crate) dialog_mode: RwSignal<Option<RegistryPresetDialogKind>>,
    pub(crate) form_error: RwSignal<Option<String>>,
    pub(crate) new_api_base: RwSignal<String>,
    pub(crate) new_label: RwSignal<String>,
    pub(crate) new_model_id: RwSignal<String>,
    pub(crate) new_api_key: RwSignal<String>,
    pub(crate) new_ctx_tokens: RwSignal<String>,
    pub(crate) new_temperature: RwSignal<String>,
    pub(crate) new_thinking_mode: RwSignal<String>,
    pub(crate) sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    pub(crate) llm_settings_feedback: RwSignal<Option<String>>,
    pub(crate) pending_delete_row_key: RwSignal<Option<String>>,
}

#[derive(Clone)]
struct RegistryPresetRowModel {
    /// 列表重排后仍与 `saved_model_presets` 下标一致（勿用手动 `enumerate` 的固定 `usize`）。
    row_index: ReadSignal<usize>,
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
    } = s;
    let sync_baseline_sv = StoredValue::new(sync_saved_presets_baseline.clone());
    let sync_for_toggle = sync_saved_presets_baseline.clone();
    let RegistryPresetRowModel { row_index, preset } = row;
    let show_delete_confirm = Memo::new(move |_| {
        let pending = pending_delete_row_key.get();
        let i = row_index.get();
        let v = saved_model_presets.get();
        let rk = v.get(i).map(saved_preset_row_key);
        matches!((pending, rk), (Some(pk), Some(rk)) if pk == rk)
    });
    let label = preset.label.clone();
    let base_short = preset.api_base.clone();
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
            <div class="settings-model-registry-primary">
                <span class="settings-saved-models-label">{label}</span>
                <div class="settings-model-registry-inline-actions">
                <button
                    type="button"
                    class="settings-model-toggle"
                    role="switch"
                    class:settings-model-toggle-on=move || {
                        let idx = row_index.get();
                        saved_model_presets
                            .get()
                            .get(idx)
                            .is_some_and(|p| p.enabled)
                    }
                    prop:aria-checked=move || {
                        let idx = row_index.get();
                        saved_model_presets
                            .get()
                            .get(idx)
                            .is_some_and(|p| p.enabled)
                    }
                    prop:aria-label=move || i18n::settings_models_row_enabled_aria(locale.get())
                    prop:title=move || i18n::settings_models_row_enabled_short(locale.get())
                    on:click=move |_| {
                        pending_delete_row_key.set(None);
                        let loc = locale.get_untracked();
                        let idx = row_index.get_untracked();
                        let mut next = saved_model_presets.with_untracked(|v| v.clone());
                        if let Some(p) = next.get_mut(idx) {
                            p.enabled = !p.enabled;
                        } else {
                            return;
                        }
                        let _ = try_persist_saved_presets_with_feedback(
                            next,
                            loc,
                            saved_model_presets,
                            &sync_for_toggle,
                            llm_settings_feedback,
                        );
                    }
                >
                    <span class="settings-model-toggle-track" aria-hidden="true">
                    <span class="settings-model-toggle-thumb"></span>
                    </span>
                </button>
                <button
                    type="button"
                    class="btn btn-ghost settings-model-registry-edit"
                    prop:aria-label=move || i18n::settings_models_row_edit_aria(locale.get())
                    prop:title=move || i18n::settings_models_row_edit_btn(locale.get())
                    on:click=move |_| {
                        pending_delete_row_key.set(None);
                        let idx = row_index.get_untracked();
                        let Some(p) = saved_model_presets.with_untracked(|v| v.get(idx).cloned())
                        else {
                            return;
                        };
                        new_label.set(p.label);
                        new_api_base.set(p.api_base);
                        new_model_id.set(p.model);
                        new_api_key.set(p.api_key);
                        new_ctx_tokens.set(p.llm_context_tokens);
                        new_temperature.set(p.temperature);
                        new_thinking_mode.set(p.llm_thinking_mode);
                        dialog_mode.set(Some(RegistryPresetDialogKind::Edit(idx)));
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
                        <path d="M12 20h9" />
                        <path d="M16.5 3.5a2.121 2.121 0 0 1 3 3L8 18l-4 1 1-4Z" />
                    </svg>
                </button>
                <Show
                    when=move || show_delete_confirm.get()
                    fallback=move || {
                        let loc_fb = locale;
                        let row_index_fb = row_index;
                        let saved_fb = saved_model_presets;
                        let pending_fb = pending_delete_row_key;
                        view! {
                <button
                    type="button"
                    class="btn btn-ghost settings-model-registry-edit settings-model-registry-trash"
                    prop:aria-label=move || i18n::settings_saved_models_remove(loc_fb.get())
                    prop:title=move || i18n::settings_models_delete_confirm(loc_fb.get())
                    on:click=move |_| {
                        let i = row_index_fb.get_untracked();
                        let Some(k) = saved_fb.with_untracked(|v| v.get(i).map(saved_preset_row_key))
                        else {
                            return;
                        };
                        pending_fb.set(Some(k));
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
                        <path d="M3 6h18" />
                        <path d="M8 6V4h8v2" />
                        <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6" />
                        <path d="M10 11v6M14 11v6" />
                    </svg>
                </button>
                        }
                    }
                >
                    <SettingsModelsDeleteConfirm
                        locale
                        saved_model_presets
                        row_index
                        sync_saved_presets_baseline={sync_baseline_sv.get_value().clone()}
                        llm_settings_feedback
                        pending_delete_row_key
                    />
                </Show>
                </div>
            </div>
            <div class="settings-model-registry-secondary">
                <div class="settings-model-registry-meta-block">
                    <span class="settings-model-registry-meta">{base_short}</span>
                    {ctx_meta.map(|line| view! {
                        <span class="settings-model-registry-meta">{line}</span>
                    })}
                </div>
            </div>
        </li>
    }
}

#[derive(Clone)]
pub(crate) struct RegistryPresetListSignals {
    pub(crate) locale: RwSignal<Locale>,
    pub(crate) saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    pub(crate) dialog_mode: RwSignal<Option<RegistryPresetDialogKind>>,
    pub(crate) form_error: RwSignal<Option<String>>,
    pub(crate) new_api_base: RwSignal<String>,
    pub(crate) new_label: RwSignal<String>,
    pub(crate) new_model_id: RwSignal<String>,
    pub(crate) new_api_key: RwSignal<String>,
    pub(crate) new_ctx_tokens: RwSignal<String>,
    pub(crate) new_temperature: RwSignal<String>,
    pub(crate) new_thinking_mode: RwSignal<String>,
    pub(crate) sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    pub(crate) llm_settings_feedback: RwSignal<Option<String>>,
    pub(crate) pending_delete_row_key: RwSignal<Option<String>>,
}

#[component]
pub(crate) fn SettingsModelsRegistryPresetList(s: RegistryPresetListSignals) -> impl IntoView {
    let saved_model_presets = s.saved_model_presets;
    let row_sig = RegistryPresetRowSignals {
        locale: s.locale,
        saved_model_presets,
        dialog_mode: s.dialog_mode,
        form_error: s.form_error,
        new_api_base: s.new_api_base,
        new_label: s.new_label,
        new_model_id: s.new_model_id,
        new_api_key: s.new_api_key,
        new_ctx_tokens: s.new_ctx_tokens,
        new_temperature: s.new_temperature,
        new_thinking_mode: s.new_thinking_mode,
        sync_saved_presets_baseline: s.sync_saved_presets_baseline.clone(),
        llm_settings_feedback: s.llm_settings_feedback,
        pending_delete_row_key: s.pending_delete_row_key,
    };
    view! {
        <ul class="settings-saved-models-list" role="list">
            <ForEnumerate
                each=move || saved_model_presets.get()
                key=|p| saved_preset_row_key(p)
                children=move |row_index, preset| {
                    view! {
                        <SettingsModelsRegistryPresetRow
                            s=row_sig.clone()
                            row=RegistryPresetRowModel { row_index, preset }
                        />
                    }
                }
            />
        </ul>
    }
}
