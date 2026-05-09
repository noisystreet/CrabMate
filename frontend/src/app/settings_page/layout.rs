//! 设置页导航轨与内容区（从 `settings_page` 拆出以降低 `SettingsPageView` 的 nloc 棘轮）。

use leptos::prelude::*;

use crate::api::{ExecutorLlmDraftSignals, MainLlmDraftSignals};

use super::super::settings_models_registry::{
    SettingsModelsRegistryBundle, SettingsModelsRegistryPanel,
};
use super::super::settings_sections::{
    SettingsAppearanceBlock, SettingsExecutorLlmBlock, SettingsLlmBlock, SettingsLlmBlockBundle,
    SettingsShortcutsBlock, SettingsToolsBlock,
};
use super::hash_routing::{SettingsSection, write_settings_section_to_hash};
use super::section_copy::{section_desc, section_title};
use crate::i18n::{self, Locale};

#[component]
pub(super) fn SettingsPageNavRail(
    active_section: RwSignal<SettingsSection>,
    appearance_locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <nav class="settings-nav" prop:aria-label=move || i18n::settings_nav_aria(appearance_locale.get())>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Appearance
                on:click=move |_| {
                    active_section.set(SettingsSection::Appearance);
                    write_settings_section_to_hash(SettingsSection::Appearance);
                }
            >
                {move || i18n::settings_section_appearance_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Llm
                on:click=move |_| {
                    active_section.set(SettingsSection::Llm);
                    write_settings_section_to_hash(SettingsSection::Llm);
                }
            >
                {move || i18n::settings_section_llm_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::ExecutorLlm
                on:click=move |_| {
                    active_section.set(SettingsSection::ExecutorLlm);
                    write_settings_section_to_hash(SettingsSection::ExecutorLlm);
                }
            >
                {move || i18n::settings_section_executor_llm_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Tools
                on:click=move |_| {
                    active_section.set(SettingsSection::Tools);
                    write_settings_section_to_hash(SettingsSection::Tools);
                }
            >
                {move || i18n::settings_section_tools_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Shortcuts
                on:click=move |_| {
                    active_section.set(SettingsSection::Shortcuts);
                    write_settings_section_to_hash(SettingsSection::Shortcuts);
                }
            >
                {move || i18n::settings_section_shortcuts_title(appearance_locale.get())}
            </button>
        </nav>
    }
}

/// 设置内容区各块共用的草稿信号（缩短 `SettingsPageContentPanels` 形参列表）。
#[derive(Clone, Copy)]
pub(super) struct SettingsPagePanelDrafts {
    pub appearance_theme: RwSignal<String>,
    pub appearance_bg_decor: RwSignal<bool>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
}

#[component]
pub(super) fn SettingsPageContentPanels(
    active_section: RwSignal<SettingsSection>,
    appearance_locale: RwSignal<Locale>,
    drafts: SettingsPagePanelDrafts,
    clear_client_key_intent: RwSignal<bool>,
    clear_executor_key_intent: RwSignal<bool>,
    execution_mode_draft: RwSignal<String>,
    readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
) -> impl IntoView {
    let SettingsPagePanelDrafts {
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
    } = drafts;

    view! {
        <section class="settings-content">
            <header class="settings-content-header">
                <h2 class="settings-content-title">{move || section_title(active_section.get(), appearance_locale.get())}</h2>
                <p class="settings-content-desc">{move || section_desc(active_section.get(), appearance_locale.get())}</p>
            </header>
            <Show when=move || active_section.get() == SettingsSection::Appearance>
                <SettingsAppearanceBlock
                    locale=appearance_locale
                    appearance_locale=appearance_locale
                    appearance_theme=appearance_theme
                    appearance_bg_decor=appearance_bg_decor
                    theme_select_id="settings-page-appearance-theme"
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Llm>
                <SettingsModelsRegistryPanel bundle=SettingsModelsRegistryBundle {
                    locale: appearance_locale,
                    saved_model_presets,
                    main: MainLlmDraftSignals {
                        llm_api_base_draft,
                        llm_api_base_preset_select,
                        llm_model_draft,
                        llm_temperature_draft,
                        llm_context_tokens_draft,
                        llm_thinking_mode_draft,
                    },
                    main_api_key_draft: llm_api_key_draft,
                    exec: ExecutorLlmDraftSignals {
                        executor_llm_api_base_draft,
                        executor_llm_api_base_preset_select,
                        executor_llm_model_draft,
                    },
                    exec_api_key_draft: executor_llm_api_key_draft,
                    form_id_prefix: "settings-page",
                } />
                <SettingsLlmBlock bundle=SettingsLlmBlockBundle {
                    locale: appearance_locale,
                    llm_api_base_draft,
                    llm_api_base_preset_select,
                    llm_model_draft,
                    llm_temperature_draft,
                    llm_context_tokens_draft,
                    llm_thinking_mode_draft,
                    execution_mode_draft: Some(execution_mode_draft),
                    llm_api_key_draft,
                    llm_has_saved_key,
                    clear_client_key_intent,
                    hint_class: "settings-field-nested-hint",
                    llm_thinking_mode_select_id: "settings-page-llm-thinking-mode",
                } />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::ExecutorLlm>
                <SettingsExecutorLlmBlock
                    locale=appearance_locale
                    executor_llm_api_base_draft=executor_llm_api_base_draft
                    executor_llm_api_base_preset_select=executor_llm_api_base_preset_select
                    executor_llm_model_draft=executor_llm_model_draft
                    executor_llm_api_key_draft=executor_llm_api_key_draft
                    executor_llm_has_saved_key=executor_llm_has_saved_key
                    clear_executor_key_intent=clear_executor_key_intent
                    hint_class="settings-field-nested-hint"
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Tools>
                <SettingsToolsBlock
                    locale=appearance_locale
                    readonly_tool_ttl_cache_follow_server=readonly_tool_ttl_cache_follow_server
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Shortcuts>
                <SettingsShortcutsBlock
                    locale=appearance_locale
                    body_class="settings-intro"
                />
            </Show>
        </section>
    }
}
