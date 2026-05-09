//! 侧栏「已保存模型」快捷列表：与现有扁平 `client_llm.*` 并存，用于下拉框填充草稿。

use leptos::prelude::*;
use serde::{Deserialize, Serialize};

use super::browser::local_storage;

const STORAGE_KEY: &str = "crabmate-web-saved-model-presets-v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedModelPreset {
    pub label: String,
    pub api_base: String,
    pub api_base_preset_select: String,
    pub model: String,
    pub temperature: String,
    pub llm_context_tokens: String,
    pub llm_thinking_mode: String,
    /// 可选；旧版列表无该字段时反序列化为空串。
    #[serde(default)]
    pub api_key: String,
}

#[must_use]
pub fn load_saved_model_presets_from_storage() -> Vec<SavedModelPreset> {
    let Some(st) = local_storage() else {
        return Vec::new();
    };
    let Ok(Some(raw)) = st.get_item(STORAGE_KEY) else {
        return Vec::new();
    };
    let t = raw.trim();
    if t.is_empty() {
        return Vec::new();
    }
    serde_json::from_str::<Vec<SavedModelPreset>>(t).unwrap_or_default()
}

pub fn persist_saved_model_presets_to_storage(
    presets: &[SavedModelPreset],
    loc: crate::i18n::Locale,
) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let json = serde_json::to_string(presets).map_err(|e| e.to_string())?;
    st.set_item(STORAGE_KEY, &json)
        .map_err(|_| "写入已保存模型列表失败".to_string())
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

/// 从当前主 LLM 草稿生成一条可入库的预设（标签可与模型名对齐）；`api_key` 取当前主模型密钥草稿。
#[must_use]
pub fn saved_model_preset_from_main_drafts(
    llm_api_base_draft: &str,
    llm_api_base_preset_select: &str,
    llm_model_draft: &str,
    llm_temperature_draft: &str,
    llm_context_tokens_draft: &str,
    llm_thinking_mode_draft: &str,
    api_key: &str,
) -> SavedModelPreset {
    let model = llm_model_draft.trim();
    let label = if model.is_empty() {
        "model".to_string()
    } else {
        model.to_string()
    };
    SavedModelPreset {
        label,
        api_base: llm_api_base_draft.trim().to_string(),
        api_base_preset_select: llm_api_base_preset_select.trim().to_string(),
        model: llm_model_draft.trim().to_string(),
        temperature: llm_temperature_draft.trim().to_string(),
        llm_context_tokens: llm_context_tokens_draft.trim().to_string(),
        llm_thinking_mode: llm_thinking_mode_draft.trim().to_string(),
        api_key: api_key.to_string(),
    }
}
