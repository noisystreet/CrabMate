//! 设置页导航轨与内容区（从 `settings_page` 拆出以降低 `SettingsPageView` 的 nloc 棘轮）。

use std::sync::Arc;

use leptos::prelude::*;

use super::super::settings_mcp_block::SettingsMcpBlock;
use super::super::settings_models_registry::{
    SettingsModelsRegistryBundle, SettingsModelsRegistryPanel,
};
use super::super::settings_sections::{
    SettingsAppearanceBlock, SettingsExecutorLlmBlock, SettingsExecutorLlmBlockBundle,
    SettingsLlmBlock, SettingsLlmBlockBundle, SettingsSessionBlock,
    SettingsSessionTypographyBundle, SettingsShortcutsBlock, SettingsToolsBlock,
};
use super::hash_routing::{SettingsSection, write_settings_section_to_hash};
use super::section_copy::{section_desc, section_title};
use crate::i18n::{self, Locale};

#[component]
fn SettingsNavItem(
    active_section: RwSignal<SettingsSection>,
    section: SettingsSection,
    testid: Option<&'static str>,
    children: Children,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class="settings-nav-item"
            data-testid=testid
            class:active=move || active_section.get() == section
            on:click=move |_| {
                active_section.set(section);
                write_settings_section_to_hash(section);
            }
        >
            {children()}
        </button>
    }
}

#[component]
pub(super) fn SettingsPageNavRail(
    active_section: RwSignal<SettingsSection>,
    appearance_locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <nav class="settings-nav" prop:aria-label=move || i18n::settings_nav_aria(appearance_locale.get())>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::Appearance
                testid=Some("settings-nav-appearance")
            >
                {move || i18n::settings_section_appearance_title(appearance_locale.get())}
            </SettingsNavItem>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::Llm
                testid=Some("settings-nav-llm")
            >
                {move || i18n::settings_section_llm_title(appearance_locale.get())}
            </SettingsNavItem>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::ExecutorLlm
                testid=None
            >
                {move || i18n::settings_section_executor_llm_title(appearance_locale.get())}
            </SettingsNavItem>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::Tools
                testid=Some("settings-nav-tools")
            >
                {move || i18n::settings_section_tools_title(appearance_locale.get())}
            </SettingsNavItem>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::Mcp
                testid=Some("settings-nav-mcp")
            >
                {move || i18n::settings_section_mcp_title(appearance_locale.get())}
            </SettingsNavItem>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::Session
                testid=Some("settings-nav-session")
            >
                {move || i18n::settings_section_session_title(appearance_locale.get())}
            </SettingsNavItem>
            <SettingsNavItem
                active_section=active_section
                section=SettingsSection::Shortcuts
                testid=None
            >
                {move || i18n::settings_section_shortcuts_title(appearance_locale.get())}
            </SettingsNavItem>
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
    pub session_ui_font: RwSignal<String>,
    pub session_chat_font: RwSignal<String>,
}

/// 已保存模型列表与本机持久化回调 + 顶栏 LLM 反馈 + 会话存储切换句柄（缩短 `SettingsPageContentPanels` 形参，满足 fn-param 棘轮）。
#[derive(Clone)]
pub(super) struct SettingsPageContentRegistryWire {
    pub sync_saved_presets_baseline: Arc<dyn Fn() + Send + Sync>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub status_data: RwSignal<Option<crate::api::StatusData>>,
    pub refresh_status: Arc<dyn Fn() + Send + Sync>,
    pub session_switch_feedback: RwSignal<Option<String>>,
    pub session_switch_busy: RwSignal<bool>,
}

#[component]
pub(super) fn SettingsPageContentPanels(
    active_section: RwSignal<SettingsSection>,
    appearance_locale: RwSignal<Locale>,
    drafts: SettingsPagePanelDrafts,
    clear_client_key_intent: RwSignal<bool>,
    clear_executor_key_intent: RwSignal<bool>,
    readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
    registry_wire: SettingsPageContentRegistryWire,
) -> impl IntoView {
    let SettingsPageContentRegistryWire {
        sync_saved_presets_baseline,
        llm_settings_feedback,
        status_data,
        refresh_status,
        session_switch_feedback,
        session_switch_busy,
    } = registry_wire;
    let refresh_status_cell = StoredValue::new(refresh_status);
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
        session_ui_font,
        session_chat_font,
    } = drafts;

    let sync_saved_presets_line = StoredValue::new(sync_saved_presets_baseline);

    view! {
        <section class="settings-content">
            <header class="settings-content-header">
                <h2 class="settings-content-title">{move || section_title(active_section.get(), appearance_locale.get())}</h2>
                <Show when=move || {
                    !section_desc(active_section.get(), appearance_locale.get()).is_empty()
                }>
                    <p class="settings-content-desc">{move || {
                        section_desc(active_section.get(), appearance_locale.get())
                    }}</p>
                </Show>
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
                    form_id_prefix: "settings-page",
                    sync_saved_presets_baseline: sync_saved_presets_line.get_value().clone(),
                    llm_settings_feedback,
                } />
                <SettingsLlmBlock bundle=SettingsLlmBlockBundle {
                    locale: appearance_locale,
                    saved_model_presets,
                    llm_api_base_draft,
                    llm_api_base_preset_select,
                    llm_model_draft,
                    llm_temperature_draft,
                    llm_context_tokens_draft,
                    llm_thinking_mode_draft,
                    llm_api_key_draft,
                    llm_has_saved_key,
                    clear_client_key_intent,
                    llm_thinking_mode_select_id: "settings-page-llm-thinking-mode",
                    llm_saved_preset_select_id: "settings-page-llm-saved-preset",
                } />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::ExecutorLlm>
                <SettingsModelsRegistryPanel bundle=SettingsModelsRegistryBundle {
                    locale: appearance_locale,
                    saved_model_presets,
                    form_id_prefix: "settings-page-exec-models",
                    sync_saved_presets_baseline: sync_saved_presets_line.get_value().clone(),
                    llm_settings_feedback,
                } />
                <SettingsExecutorLlmBlock bundle=SettingsExecutorLlmBlockBundle {
                    locale: appearance_locale,
                    saved_model_presets,
                    executor_llm_api_base_draft,
                    executor_llm_api_base_preset_select,
                    executor_llm_model_draft,
                    executor_llm_api_key_draft,
                    executor_llm_has_saved_key,
                    clear_executor_key_intent,
                    executor_saved_preset_select_id: "settings-page-executor-saved-preset",
                } />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Tools>
                <SettingsToolsBlock
                    locale=appearance_locale
                    readonly_tool_ttl_cache_follow_server=readonly_tool_ttl_cache_follow_server
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Mcp>
                <SettingsMcpBlock locale=appearance_locale />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Session>
                <SettingsSessionBlock
                    locale=appearance_locale
                    status_data=status_data
                    refresh_status=refresh_status_cell.get_value()
                    session_switch_feedback=session_switch_feedback
                    session_switch_busy=session_switch_busy
                    typography=SettingsSessionTypographyBundle {
                        session_ui_font,
                        session_chat_font,
                        ui_select_id: "settings-page-session-ui-font",
                        chat_select_id: "settings-page-session-chat-font",
                    }
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
