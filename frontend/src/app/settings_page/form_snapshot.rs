//! 设置页表单快照（tracked / untracked），供 dirty 检测与 baseline 刷新。

use leptos::prelude::*;

use super::super::settings_form_state::SettingsFormCurrent;
use crate::i18n::Locale;

/// 设置页「草稿」相关 `RwSignal`（`Copy`），用于组装 [`SettingsFormCurrent`]。
#[derive(Clone, Copy)]
pub(crate) struct SettingsPageDraftSignals {
    pub appearance_locale: RwSignal<Locale>,
    pub appearance_theme: RwSignal<String>,
    pub appearance_bg_decor: RwSignal<bool>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub execution_mode_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub clear_client_key_intent: RwSignal<bool>,
    pub clear_executor_key_intent: RwSignal<bool>,
    pub llm_api_key_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
}

pub(crate) fn form_current_tracked(s: SettingsPageDraftSignals) -> SettingsFormCurrent {
    SettingsFormCurrent {
        appearance_locale: s.appearance_locale.get(),
        appearance_theme: s.appearance_theme.get(),
        appearance_bg_decor: s.appearance_bg_decor.get(),
        llm_api_base_draft: s.llm_api_base_draft.get(),
        llm_api_base_preset_select: s.llm_api_base_preset_select.get(),
        llm_model_draft: s.llm_model_draft.get(),
        llm_temperature_draft: s.llm_temperature_draft.get(),
        llm_context_tokens_draft: s.llm_context_tokens_draft.get(),
        llm_thinking_mode_draft: s.llm_thinking_mode_draft.get(),
        execution_mode_draft: s.execution_mode_draft.get(),
        llm_has_saved_key: s.llm_has_saved_key.get(),
        executor_llm_api_base_draft: s.executor_llm_api_base_draft.get(),
        executor_llm_api_base_preset_select: s.executor_llm_api_base_preset_select.get(),
        executor_llm_model_draft: s.executor_llm_model_draft.get(),
        executor_llm_has_saved_key: s.executor_llm_has_saved_key.get(),
        clear_client_key_intent: s.clear_client_key_intent.get(),
        clear_executor_key_intent: s.clear_executor_key_intent.get(),
        llm_api_key_draft: s.llm_api_key_draft.get(),
        executor_llm_api_key_draft: s.executor_llm_api_key_draft.get(),
        readonly_tool_ttl_cache_follow_server: s.readonly_tool_ttl_cache_follow_server.get(),
    }
}

pub(crate) fn form_current_untracked(s: SettingsPageDraftSignals) -> SettingsFormCurrent {
    SettingsFormCurrent {
        appearance_locale: s.appearance_locale.get_untracked(),
        appearance_theme: s.appearance_theme.get_untracked(),
        appearance_bg_decor: s.appearance_bg_decor.get_untracked(),
        llm_api_base_draft: s.llm_api_base_draft.get_untracked(),
        llm_api_base_preset_select: s.llm_api_base_preset_select.get_untracked(),
        llm_model_draft: s.llm_model_draft.get_untracked(),
        llm_temperature_draft: s.llm_temperature_draft.get_untracked(),
        llm_context_tokens_draft: s.llm_context_tokens_draft.get_untracked(),
        llm_thinking_mode_draft: s.llm_thinking_mode_draft.get_untracked(),
        execution_mode_draft: s.execution_mode_draft.get_untracked(),
        llm_has_saved_key: s.llm_has_saved_key.get_untracked(),
        executor_llm_api_base_draft: s.executor_llm_api_base_draft.get_untracked(),
        executor_llm_api_base_preset_select: s.executor_llm_api_base_preset_select.get_untracked(),
        executor_llm_model_draft: s.executor_llm_model_draft.get_untracked(),
        executor_llm_has_saved_key: s.executor_llm_has_saved_key.get_untracked(),
        clear_client_key_intent: s.clear_client_key_intent.get_untracked(),
        clear_executor_key_intent: s.clear_executor_key_intent.get_untracked(),
        llm_api_key_draft: s.llm_api_key_draft.get_untracked(),
        executor_llm_api_key_draft: s.executor_llm_api_key_draft.get_untracked(),
        readonly_tool_ttl_cache_follow_server: s
            .readonly_tool_ttl_cache_follow_server
            .get_untracked(),
    }
}
