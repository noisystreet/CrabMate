//! 侧栏「已保存模型」：存在 **`llm_overrides.saved_models`**（`/user-data/llm-overrides`）。

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use super::client_llm_cache::{with_mem, with_mem_mut};
use super::client_llm_storage;

fn default_preset_enabled() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedModelPreset {
    pub label: String,
    pub api_base: String,
    pub api_base_preset_select: String,
    pub model: String,
    pub temperature: String,
    pub llm_context_tokens: String,
    pub llm_thinking_mode: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_preset_enabled")]
    pub enabled: bool,
}

#[must_use]
pub fn load_saved_model_presets_from_storage() -> Vec<SavedModelPreset> {
    with_mem(|m| {
        m.saved_models
            .iter()
            .filter_map(|v| serde_json::from_value::<SavedModelPreset>(v.clone()).ok())
            .collect()
    })
}

pub fn persist_saved_model_presets_to_storage(
    presets: &[SavedModelPreset],
    loc: crate::i18n::Locale,
) -> Result<(), String> {
    let vals: Vec<serde_json::Value> = presets
        .iter()
        .filter_map(|p| serde_json::to_value(p).ok())
        .collect();
    with_mem_mut(|m| m.saved_models = vals);
    client_llm_storage::flush_llm_overrides_to_server(loc);
    Ok(())
}

/// 将一条已保存预设应用到「主 LLM」草稿（不含 API Key）。
pub fn apply_saved_model_preset_to_main_fields(
    preset: &SavedModelPreset,
    drafts: MainLlmDraftSignals,
) {
    let MainLlmDraftSignals {
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
    } = drafts;
    llm_api_base_draft.set(preset.api_base.clone());
    llm_api_base_preset_select.set(preset.api_base_preset_select.clone());
    llm_model_draft.set(preset.model.clone());
    llm_temperature_draft.set(preset.temperature.clone());
    llm_context_tokens_draft.set(preset.llm_context_tokens.clone());
    llm_thinking_mode_draft.set(preset.llm_thinking_mode.clone());
}

#[derive(Clone, Copy)]
pub struct MainLlmDraftSignals {
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
}

/// 将一条已保存预设应用到「执行器 LLM」草稿（网关与模型；不含 API Key）。
pub fn apply_saved_model_preset_to_executor_fields(
    preset: &SavedModelPreset,
    drafts: ExecutorLlmDraftSignals,
) {
    let ExecutorLlmDraftSignals {
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
    } = drafts;
    executor_llm_api_base_draft.set(preset.api_base.clone());
    executor_llm_api_base_preset_select.set(preset.api_base_preset_select.clone());
    executor_llm_model_draft.set(preset.model.clone());
}

#[derive(Clone, Copy)]
pub struct ExecutorLlmDraftSignals {
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
}

/// 查找与给定 `api_base` / `api_base_preset_select` / `model` 完全一致的首条已保存预设下标。
#[must_use]
pub fn matching_saved_preset_index(
    presets: &[SavedModelPreset],
    api_base: &str,
    api_base_preset_select: &str,
    model: &str,
) -> Option<usize> {
    presets.iter().position(|p| {
        p.enabled
            && p.api_base.trim() == api_base.trim()
            && p.api_base_preset_select.trim() == api_base_preset_select.trim()
            && p.model.trim() == model.trim()
    })
}
