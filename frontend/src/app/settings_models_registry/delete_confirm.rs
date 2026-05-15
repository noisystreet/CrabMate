//! 行内删除确认按钮组（从 `preset_list` 拆出以降低 `SettingsModelsRegistryPresetRow` 圈复杂度）。
//!
//! 替代部分环境下无响应的 `window.confirm`，使用 Leptos 响应式信号 + 行内确认 UI。

use std::sync::Arc;

use leptos::prelude::*;

use crate::api::SavedModelPreset;
use crate::i18n::{self, Locale};

use super::persist::try_persist_saved_presets_with_feedback;

/// 删除确认按钮组：显示「确认移除」+「取消」两个按钮。
///
/// 由 `pending_delete_row_key` 信号控制显隐（父组件 `Show` 包裹）。
#[component]
pub(super) fn SettingsModelsDeleteConfirm(
    locale: RwSignal<Locale>,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    row_index: ReadSignal<usize>,
    sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    llm_settings_feedback: RwSignal<Option<String>>,
    pending_delete_row_key: RwSignal<Option<String>>,
) -> impl IntoView {
    let sync_sv = StoredValue::new(sync_saved_presets_baseline.clone());
    view! {
        <span
            class="settings-model-delete-confirm"
            role="group"
            prop:aria-label=move || i18n::settings_models_delete_confirm(locale.get())
        >
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                on:click=move |_| {
                    let sync = sync_sv.get_value().clone();
                    let loc = locale.get_untracked();
                    let idx = row_index.get_untracked();
                    let mut next = saved_model_presets.with_untracked(|v| v.clone());
                    if idx >= next.len() {
                        pending_delete_row_key.set(None);
                        return;
                    }
                    let remove_key = super::preset_list::saved_preset_row_key(&next[idx]);
                    next.retain(|p| super::preset_list::saved_preset_row_key(p) != remove_key);
                    pending_delete_row_key.set(None);
                    let _ = try_persist_saved_presets_with_feedback(
                        next,
                        loc,
                        saved_model_presets,
                        &sync,
                        llm_settings_feedback,
                    );
                }
            >
                {move || i18n::settings_models_delete_apply_btn(locale.get())}
            </button>
            <button
                type="button"
                class="btn btn-ghost btn-sm"
                on:click=move |_| {
                    pending_delete_row_key.set(None);
                }
            >
                {move || i18n::settings_models_cancel_form(locale.get())}
            </button>
        </span>
    }
}
