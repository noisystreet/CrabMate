//! 设置页全屏视图（`SettingsPageView`）；路由与布局见同目录子模块。

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use super::effects::{
    SettingsPageOpenBaselineWire, wire_settings_page_dom_preview_effect,
    wire_settings_page_hash_section_effect, wire_settings_page_open_snapshot_effect,
};
use super::form_snapshot::{
    SettingsPageDraftSignals, form_current_tracked, form_current_untracked,
};
use super::hash_routing::{
    SettingsSection, read_settings_section_from_hash, settings_page_install_hashchange_listener,
};
use super::header::SettingsPageHeader;
use super::layout::{
    SettingsPageContentPanels, SettingsPageContentRegistryWire, SettingsPageNavRail,
    SettingsPagePanelDrafts,
};
use super::page_actions::{
    DiscardToBaselinesCtx, SaveAllSettingsCtx, discard_to_baselines, try_save_all_settings,
};
use crate::app::settings_form_state::{
    SettingsDirtyBaselines, SettingsFormCurrent, SettingsFormUiPhase, derive_settings_form_ui_phase,
};
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
        }
    }
}

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
    let SettingsPageFormSignals {
        locale,
        theme,
        bg_decor,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        llm_settings_feedback,
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
        executor_llm_api_key_draft,
        executor_llm_has_saved_key,
        executor_llm_settings_feedback,
        execution_mode_draft,
        client_llm_storage_tick,
        readonly_tool_ttl_cache_follow_server,
        saved_model_presets,
    } = form;

    let active_section =
        RwSignal::new(read_settings_section_from_hash().unwrap_or(SettingsSection::Appearance));

    let appearance_locale = RwSignal::new(locale.get_untracked());
    let appearance_theme = RwSignal::new(theme.get_untracked());
    let appearance_bg_decor = RwSignal::new(bg_decor.get_untracked());

    let clear_client_key_intent = RwSignal::new(false);
    let clear_executor_key_intent = RwSignal::new(false);

    let drafts = SettingsPageDraftSignals {
        appearance_locale,
        appearance_theme,
        appearance_bg_decor,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
        execution_mode_draft,
        llm_has_saved_key,
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
        executor_llm_has_saved_key,
        clear_client_key_intent,
        clear_executor_key_intent,
        llm_api_key_draft,
        executor_llm_api_key_draft,
        readonly_tool_ttl_cache_follow_server,
        saved_model_presets,
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
            locale,
            theme,
            bg_decor,
            appearance_locale,
            appearance_theme,
            appearance_bg_decor,
            drafts,
            clear_client_key_intent,
            clear_executor_key_intent,
            llm_settings_feedback,
            executor_llm_settings_feedback,
        },
        baselines,
    );
    wire_settings_page_dom_preview_effect(
        settings_page,
        theme,
        bg_decor,
        appearance_theme,
        appearance_bg_decor,
    );

    let dirty = Memo::new(move |_| {
        let current: SettingsFormCurrent = form_current_tracked(drafts);
        matches!(
            derive_settings_form_ui_phase(&current, &baselines),
            SettingsFormUiPhase::Dirty
        )
    });

    let discard_rc: Rc<dyn Fn()> = Rc::new(move || {
        discard_to_baselines(DiscardToBaselinesCtx {
            baselines,
            drafts,
            llm_settings_feedback,
            executor_llm_settings_feedback,
        });
    });

    let save_rc: Rc<dyn Fn()> = {
        let dirty = dirty;
        Rc::new(move || {
            try_save_all_settings(SaveAllSettingsCtx {
                dirty,
                appearance_locale,
                locale,
                theme,
                bg_decor,
                drafts,
                llm_settings_feedback,
                executor_llm_settings_feedback,
                client_llm_storage_tick,
                baselines,
            });
        })
    };

    let on_back: Rc<dyn Fn()> = {
        let dirty = dirty;
        let discard_rc = Rc::clone(&discard_rc);
        Rc::new(move || {
            if dirty.get() {
                discard_rc();
            }
            settings_page.set(false);
        })
    };

    view! {
        <div class="settings-page" class:settings-page-visible=move || settings_page.get()>
            <SettingsPageHeader
                appearance_locale=appearance_locale
                dirty=dirty
                on_back=on_back
                on_discard=discard_rc
                on_save=save_rc
            />
            <div class="settings-page-body">
                <Show when=move || llm_settings_feedback.get().is_some()>
                    <p class="settings-save-feedback settings-save-feedback-global">{move || {
                        llm_settings_feedback.get().unwrap_or_default()
                    }}</p>
                </Show>
                <div class="settings-layout">
                    <SettingsPageNavRail active_section=active_section appearance_locale=appearance_locale />
                    <SettingsPageContentPanels
                        active_section=active_section
                        appearance_locale=appearance_locale
                        drafts=SettingsPagePanelDrafts {
                            appearance_theme,
                            appearance_bg_decor,
                            llm_api_base_draft,
                            llm_api_base_preset_select,
                            llm_model_draft,
                            llm_temperature_draft,
                            llm_context_tokens_draft,
                            llm_thinking_mode_draft,
                            llm_api_key_draft,
                            llm_has_saved_key,
                            executor_llm_api_base_draft,
                            executor_llm_api_base_preset_select,
                            executor_llm_model_draft,
                            executor_llm_api_key_draft,
                            executor_llm_has_saved_key,
                            saved_model_presets,
                        }
                        clear_client_key_intent=clear_client_key_intent
                        clear_executor_key_intent=clear_executor_key_intent
                        execution_mode_draft=execution_mode_draft
                        readonly_tool_ttl_cache_follow_server=readonly_tool_ttl_cache_follow_server
                        registry_wire=SettingsPageContentRegistryWire {
                            sync_saved_presets_baseline: sync_saved_presets_baseline.clone(),
                            llm_settings_feedback,
                            status_data,
                            refresh_status,
                            session_switch_feedback,
                            session_switch_busy,
                        }
                    />
                </div>
            </div>
        </div>
    }
}
