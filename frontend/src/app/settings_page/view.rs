//! 设置页全屏视图（`SettingsPageView`）；路由与布局见同目录子模块。

use std::sync::Arc;

use leptos::prelude::*;

use super::chrome::{SettingsPageChrome, SettingsPageChromeCtx};
use super::effects::{
    SettingsPageOpenBaselineWire, wire_settings_page_dom_preview_effect,
    wire_settings_page_hash_section_effect, wire_settings_page_open_snapshot_effect,
};
use super::form_signals::SettingsPageFormSignals;
use super::form_snapshot::{SettingsPageDraftSignals, form_current_untracked};
use super::hash_routing::{
    SettingsSection, read_settings_section_from_hash, settings_page_install_hashchange_listener,
};
use crate::app::settings_form_state::SettingsDirtyBaselines;

/// 设置页全屏视图入参（阶段 B：`App` 单行传入）。
#[derive(Clone)]
pub struct SettingsPageViewInput {
    pub settings_page: RwSignal<bool>,
    pub form: SettingsPageFormSignals,
    pub status_data: RwSignal<Option<crate::api::StatusData>>,
    pub refresh_status: std::sync::Arc<dyn Fn() + Send + Sync>,
}

#[component]
pub fn SettingsPageView(input: SettingsPageViewInput) -> impl IntoView {
    let SettingsPageViewInput {
        settings_page,
        form,
        status_data,
        refresh_status,
    } = input;

    let active_section =
        RwSignal::new(read_settings_section_from_hash().unwrap_or(SettingsSection::Appearance));

    let appearance_locale = RwSignal::new(form.locale.get_untracked());
    let appearance_theme = RwSignal::new(form.theme.get_untracked());
    let appearance_bg_decor = RwSignal::new(form.bg_decor.get_untracked());

    let clear_client_key_intent = RwSignal::new(false);
    let clear_executor_key_intent = RwSignal::new(false);

    let drafts = SettingsPageDraftSignals {
        appearance_locale,
        appearance_theme,
        appearance_bg_decor,
        llm_api_base_draft: form.llm_api_base_draft,
        llm_api_base_preset_select: form.llm_api_base_preset_select,
        llm_model_draft: form.llm_model_draft,
        llm_temperature_draft: form.llm_temperature_draft,
        llm_context_tokens_draft: form.llm_context_tokens_draft,
        llm_thinking_mode_draft: form.llm_thinking_mode_draft,
        llm_has_saved_key: form.llm_has_saved_key,
        executor_llm_api_base_draft: form.executor_llm_api_base_draft,
        executor_llm_api_base_preset_select: form.executor_llm_api_base_preset_select,
        executor_llm_model_draft: form.executor_llm_model_draft,
        executor_llm_has_saved_key: form.executor_llm_has_saved_key,
        clear_client_key_intent,
        clear_executor_key_intent,
        llm_api_key_draft: form.llm_api_key_draft,
        executor_llm_api_key_draft: form.executor_llm_api_key_draft,
        readonly_tool_ttl_cache_follow_server: form.readonly_tool_ttl_cache_follow_server,
        saved_model_presets: form.saved_model_presets,
    };

    let baselines = SettingsDirtyBaselines::from_form_current(&form_current_untracked(drafts));

    let session_switch_feedback = RwSignal::new(None::<String>);
    let session_switch_busy = RwSignal::new(false);

    let sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        baselines.refresh_from_current(&form_current_untracked(drafts));
    });

    settings_page_install_hashchange_listener(active_section);

    wire_settings_page_hash_section_effect(settings_page, active_section);
    wire_settings_page_open_snapshot_effect(
        SettingsPageOpenBaselineWire {
            settings_page,
            locale: form.locale,
            theme: form.theme,
            bg_decor: form.bg_decor,
            appearance_locale,
            appearance_theme,
            appearance_bg_decor,
            drafts,
            clear_client_key_intent,
            clear_executor_key_intent,
            llm_settings_feedback: form.llm_settings_feedback,
            executor_llm_settings_feedback: form.executor_llm_settings_feedback,
        },
        baselines,
    );
    wire_settings_page_dom_preview_effect(
        settings_page,
        form.theme,
        form.bg_decor,
        appearance_theme,
        appearance_bg_decor,
    );

    let refresh_status_sv = StoredValue::new(refresh_status);
    let sync_saved_sv = StoredValue::new(sync_saved_presets_baseline);

    view! {
        <SettingsPageChrome ctx=SettingsPageChromeCtx {
            settings_page,
            form,
            active_section,
            appearance_locale,
            appearance_theme,
            appearance_bg_decor,
            drafts,
            baselines,
            clear_client_key_intent,
            clear_executor_key_intent,
            sync_saved_presets_baseline: sync_saved_sv,
            session_switch_feedback,
            session_switch_busy,
            status_data,
            refresh_status: refresh_status_sv,
        } />
    }
}
