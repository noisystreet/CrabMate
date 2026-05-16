//! 设置页表单 `RwSignal` 聚合（供 `view` / `chrome` 共用，避免模块循环依赖）。

use leptos::prelude::*;

use crate::i18n::Locale;

/// 设置页中与 LLM / 外观相关的 `RwSignal` 聚合（缩短 `SettingsPageView` 形参列表）。
#[derive(Clone, Copy)]
pub struct SettingsPageFormSignals {
    pub locale: RwSignal<Locale>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
    pub execution_mode_draft: RwSignal<String>,
    pub client_llm_storage_tick: RwSignal<u64>,
    pub readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
    pub session_ui_font: RwSignal<String>,
    pub session_chat_font: RwSignal<String>,
}

impl SettingsPageFormSignals {
    /// 从 [`crate::app::app_signals::AppSignals`] 组装 LLM / 外观草稿信号（壳层设置页与弹窗共用）。
    #[must_use]
    pub fn from_app_signals(app: &crate::app::app_signals::AppSignals) -> Self {
        Self {
            locale: app.shell_ui.locale,
            theme: app.shell_ui.theme,
            bg_decor: app.shell_ui.bg_decor,
            llm_api_base_draft: app.llm_settings.llm_api_base_draft,
            llm_api_base_preset_select: app.llm_settings.llm_api_base_preset_select,
            llm_model_draft: app.llm_settings.llm_model_draft,
            llm_temperature_draft: app.llm_settings.llm_temperature_draft,
            llm_context_tokens_draft: app.llm_settings.llm_context_tokens_draft,
            llm_thinking_mode_draft: app.llm_settings.llm_thinking_mode_draft,
            llm_api_key_draft: app.llm_settings.llm_api_key_draft,
            llm_has_saved_key: app.llm_settings.llm_has_saved_key,
            llm_settings_feedback: app.llm_settings.llm_settings_feedback,
            executor_llm_api_base_draft: app.llm_settings.executor_llm_api_base_draft,
            executor_llm_api_base_preset_select: app
                .llm_settings
                .executor_llm_api_base_preset_select,
            executor_llm_model_draft: app.llm_settings.executor_llm_model_draft,
            executor_llm_api_key_draft: app.llm_settings.executor_llm_api_key_draft,
            executor_llm_has_saved_key: app.llm_settings.executor_llm_has_saved_key,
            executor_llm_settings_feedback: app.llm_settings.executor_llm_settings_feedback,
            execution_mode_draft: app.llm_settings.execution_mode_draft,
            client_llm_storage_tick: app.llm_settings.client_llm_storage_tick,
            readonly_tool_ttl_cache_follow_server: app
                .llm_settings
                .readonly_tool_ttl_cache_follow_server,
            saved_model_presets: app.llm_settings.saved_model_presets,
            session_ui_font: app.shell_ui.session_ui_font,
            session_chat_font: app.shell_ui.session_chat_font,
        }
    }
}
