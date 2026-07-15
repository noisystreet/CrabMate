//! 设置页主体（dirty / 保存 / 放弃 / 内容区），从 `view.rs` 拆出以降低 `SettingsPageView` 的 `nloc` 棘轮。

use std::rc::Rc;
use std::sync::Arc;

use leptos::prelude::*;

use super::form_signals::SettingsPageFormSignals;
use super::form_snapshot::{SettingsPageDraftSignals, form_current_tracked};
use super::hash_routing::SettingsSection;
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

/// `SettingsPageChrome` 所需的信号（`SettingsPageView` 在挂接 `wire_*` 后传入）。
#[derive(Clone, Copy)]
pub(crate) struct SettingsPageChromeCtx {
    pub settings_page: RwSignal<bool>,
    pub form: SettingsPageFormSignals,
    pub active_section: RwSignal<SettingsSection>,
    pub appearance_locale: RwSignal<Locale>,
    pub appearance_theme: RwSignal<String>,
    pub appearance_bg_decor: RwSignal<bool>,
    pub drafts: SettingsPageDraftSignals,
    pub baselines: SettingsDirtyBaselines,
    pub clear_client_key_intent: RwSignal<bool>,
    pub clear_executor_key_intent: RwSignal<bool>,
    pub sync_saved_presets_baseline: StoredValue<Arc<dyn Fn() + Send + Sync>>,
    pub session_switch_feedback: RwSignal<Option<String>>,
    pub session_switch_busy: RwSignal<bool>,
    pub status_data: RwSignal<Option<crate::api::StatusData>>,
    pub refresh_status: StoredValue<Arc<dyn Fn() + Send + Sync>>,
}

#[component]
pub(super) fn SettingsPageChrome(ctx: SettingsPageChromeCtx) -> impl IntoView {
    let SettingsPageChromeCtx {
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
        sync_saved_presets_baseline,
        session_switch_feedback,
        session_switch_busy,
        status_data,
        refresh_status,
    } = ctx;
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
        client_llm_storage_tick,
        readonly_tool_ttl_cache_follow_server,
        saved_model_presets,
        session_ui_font,
        session_chat_font,
    } = form;

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
        <div
            class="settings-page"
            class:settings-page-visible=move || settings_page.get()
            data-testid="settings-page"
        >
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
                            session_ui_font,
                            session_chat_font,
                        }
                        clear_client_key_intent=clear_client_key_intent
                        clear_executor_key_intent=clear_executor_key_intent
                        readonly_tool_ttl_cache_follow_server=readonly_tool_ttl_cache_follow_server
                        registry_wire=SettingsPageContentRegistryWire {
                            sync_saved_presets_baseline: sync_saved_presets_baseline.get_value().clone(),
                            llm_settings_feedback,
                            status_data,
                            refresh_status: refresh_status.get_value(),
                            session_switch_feedback,
                            session_switch_busy,
                        }
                    />
                </div>
            </div>
        </div>
    }
}
