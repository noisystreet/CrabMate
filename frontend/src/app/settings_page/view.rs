//! 设置页全屏视图（`SettingsPageView`）；路由与布局见同目录子模块。

use leptos::prelude::*;

use super::dom_preview::{apply_bg_decor_preview_to_dom, apply_theme_preview_to_dom};
use super::form_snapshot::{
    SettingsPageDraftSignals, form_current_tracked, form_current_untracked,
};
use super::hash_routing::{
    SettingsSection, clear_settings_hash_if_present, read_settings_section_from_hash,
    settings_page_install_hashchange_listener, write_settings_section_to_hash,
};
use super::layout::{SettingsPageContentPanels, SettingsPageNavRail, SettingsPagePanelDrafts};
use super::page_actions::{
    DiscardToBaselinesCtx, SaveAllSettingsCtx, discard_to_baselines, try_save_all_settings,
};
use crate::app::settings_form_state::{SettingsDirtyBaselines, SettingsFormCurrent};
use crate::i18n::{self, Locale};

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
}

/// 设置页全屏视图入参（阶段 B：`App` 单行传入）。
#[derive(Clone, Copy)]
pub struct SettingsPageViewInput {
    pub settings_page: RwSignal<bool>,
    pub form: SettingsPageFormSignals,
}

#[component]
pub fn SettingsPageView(input: SettingsPageViewInput) -> impl IntoView {
    let SettingsPageViewInput {
        settings_page,
        form,
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
    };

    let baselines = SettingsDirtyBaselines::from_form_current(&form_current_untracked(drafts));

    let current_state_untracked = move || form_current_untracked(drafts);

    settings_page_install_hashchange_listener(active_section);

    Effect::new(move |_| {
        if !settings_page.get() {
            clear_settings_hash_if_present();
            return;
        }
        write_settings_section_to_hash(active_section.get());
    });

    Effect::new(move |_| {
        if !settings_page.get() {
            return;
        }
        appearance_locale.set(locale.get_untracked());
        appearance_theme.set(theme.get_untracked());
        appearance_bg_decor.set(bg_decor.get_untracked());
        baselines.refresh_from_current(&current_state_untracked());
        clear_client_key_intent.set(false);
        clear_executor_key_intent.set(false);
        llm_settings_feedback.set(None);
        executor_llm_settings_feedback.set(None);
    });

    Effect::new(move |_| {
        if !settings_page.get() {
            apply_theme_preview_to_dom(theme.get().as_str());
            apply_bg_decor_preview_to_dom(bg_decor.get());
            return;
        }
        apply_theme_preview_to_dom(appearance_theme.get().as_str());
        apply_bg_decor_preview_to_dom(appearance_bg_decor.get());
    });

    let dirty = Memo::new(move |_| {
        let current: SettingsFormCurrent = form_current_tracked(drafts);
        baselines.is_dirty(&current)
    });

    let discard = {
        move |_| {
            discard_to_baselines(DiscardToBaselinesCtx {
                baselines,
                drafts,
                llm_settings_feedback,
                executor_llm_settings_feedback,
            });
        }
    };

    let save_all = {
        let dirty = dirty;
        move |_| {
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
        }
    };

    view! {
        <div class="settings-page" class:settings-page-visible=move || settings_page.get()>
            <div class="settings-page-header">
                <button
                    type="button"
                    class="btn btn-ghost settings-page-back"
                    on:click=move |_| {
                        if dirty.get() {
                            discard(());
                        }
                        settings_page.set(false);
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
                        <polyline points="15 18 9 12 15 6" />
                    </svg>
                    <span>{move || i18n::settings_back(appearance_locale.get())}</span>
                </button>
                <h1 class="settings-page-title">{move || i18n::settings_title(appearance_locale.get())}</h1>
                <span class="settings-page-badge">{move || i18n::settings_badge_local(appearance_locale.get())}</span>
                <Show when=move || dirty.get()>
                    <span class="settings-unsaved-pill">{move || i18n::settings_unsaved_badge(appearance_locale.get())}</span>
                </Show>
                <span class="settings-page-head-spacer"></span>
                <div class="settings-page-header-actions">
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        prop:disabled=move || !dirty.get()
                        on:click=move |_| discard(())
                    >
                        {move || i18n::settings_discard_changes(appearance_locale.get())}
                    </button>
                    <button
                        type="button"
                        class="btn btn-primary btn-sm"
                        prop:disabled=move || !dirty.get()
                        on:click=move |_| save_all(())
                    >
                        {move || i18n::settings_save_all(appearance_locale.get())}
                    </button>
                </div>
            </div>
            <div class="settings-page-body">
                <p class="settings-intro">{move || i18n::settings_intro(appearance_locale.get())}</p>
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
                        }
                        clear_client_key_intent=clear_client_key_intent
                        clear_executor_key_intent=clear_executor_key_intent
                        execution_mode_draft=execution_mode_draft
                        readonly_tool_ttl_cache_follow_server=readonly_tool_ttl_cache_follow_server
                    />
                </div>
            </div>
        </div>
    }
}
