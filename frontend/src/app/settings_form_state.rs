use leptos::prelude::*;

use crate::i18n::Locale;

pub(crate) type AppearanceBaseline = (Locale, String, bool);
pub(crate) type LlmBaseline = (String, String, String, String, String, String, String, bool);
pub(crate) type ExecutorBaseline = (String, String, String, bool);

#[derive(Clone)]
pub(crate) struct SettingsFormCurrent {
    pub appearance_locale: Locale,
    pub appearance_theme: String,
    pub appearance_bg_decor: bool,
    pub llm_api_base_draft: String,
    pub llm_api_base_preset_select: String,
    pub llm_model_draft: String,
    pub llm_temperature_draft: String,
    pub llm_context_tokens_draft: String,
    pub llm_thinking_mode_draft: String,
    pub execution_mode_draft: String,
    pub llm_has_saved_key: bool,
    pub executor_llm_api_base_draft: String,
    pub executor_llm_api_base_preset_select: String,
    pub executor_llm_model_draft: String,
    pub executor_llm_has_saved_key: bool,
    pub clear_client_key_intent: bool,
    pub clear_executor_key_intent: bool,
    pub llm_api_key_draft: String,
    pub executor_llm_api_key_draft: String,
}

pub(crate) fn is_settings_dirty(
    current: &SettingsFormCurrent,
    baseline_appearance: &AppearanceBaseline,
    baseline_llm: &LlmBaseline,
    baseline_executor: &ExecutorBaseline,
) -> bool {
    let (bl, bt, bbd) = baseline_appearance;
    if current.appearance_locale != *bl
        || current.appearance_theme != *bt
        || current.appearance_bg_decor != *bbd
    {
        return true;
    }
    if current.clear_client_key_intent || current.clear_executor_key_intent {
        return true;
    }
    if !current.llm_api_key_draft.trim().is_empty()
        || !current.executor_llm_api_key_draft.trim().is_empty()
    {
        return true;
    }

    let (bb, bp, bm, bt, bct, btm, be, bh) = baseline_llm;
    if current.llm_api_base_draft != *bb
        || current.llm_api_base_preset_select != *bp
        || current.llm_model_draft != *bm
        || current.llm_temperature_draft != *bt
        || current.llm_context_tokens_draft != *bct
        || current.llm_thinking_mode_draft != *btm
        || current.execution_mode_draft != *be
        || current.llm_has_saved_key != *bh
    {
        return true;
    }

    let (eb, ep, em, eh) = baseline_executor;
    current.executor_llm_api_base_draft != *eb
        || current.executor_llm_api_base_preset_select != *ep
        || current.executor_llm_model_draft != *em
        || current.executor_llm_has_saved_key != *eh
}

pub(crate) fn refresh_baselines(
    baseline_appearance: StoredValue<AppearanceBaseline>,
    baseline_llm: StoredValue<LlmBaseline>,
    baseline_executor: StoredValue<ExecutorBaseline>,
    current: &SettingsFormCurrent,
) {
    let _ = baseline_appearance.try_update_value(|v| {
        *v = (
            current.appearance_locale,
            current.appearance_theme.clone(),
            current.appearance_bg_decor,
        );
    });
    let _ = baseline_llm.try_update_value(|v| {
        *v = (
            current.llm_api_base_draft.clone(),
            current.llm_api_base_preset_select.clone(),
            current.llm_model_draft.clone(),
            current.llm_temperature_draft.clone(),
            current.llm_context_tokens_draft.clone(),
            current.llm_thinking_mode_draft.clone(),
            current.execution_mode_draft.clone(),
            current.llm_has_saved_key,
        );
    });
    let _ = baseline_executor.try_update_value(|v| {
        *v = (
            current.executor_llm_api_base_draft.clone(),
            current.executor_llm_api_base_preset_select.clone(),
            current.executor_llm_model_draft.clone(),
            current.executor_llm_has_saved_key,
        );
    });
}
