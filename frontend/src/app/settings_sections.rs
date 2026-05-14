use std::sync::Arc;

use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::event_target_value;
use wasm_bindgen::JsCast;

use crate::api::MainLlmDraftSignals;
use crate::app_prefs::THEME_SLUGS;
use crate::i18n::{self, Locale};
use crate::settings_llm_fields::{
    LlmContextTokensField, LlmSavedPresetApplyTarget, LlmSavedPresetPicker, LlmTemperatureField,
    LlmThinkingModeField, OptionalLlmExecutionModeField,
};

/// 设置页「主 LLM」区块所需信号（缩短 [`SettingsLlmBlock`] 形参列表；勿命名为 `*Props`，与 Leptos 组件宏生成类型冲突）。
#[derive(Clone, Copy)]
pub(crate) struct SettingsLlmBlockBundle {
    pub locale: RwSignal<Locale>,
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub execution_mode_draft: Option<RwSignal<String>>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub clear_client_key_intent: RwSignal<bool>,
    /// `<select id=…>`：设置页与弹窗可能同时挂载，须用不同 id。
    pub llm_thinking_mode_select_id: &'static str,
    /// 已保存模型下拉：设置页与弹窗须不同 id。
    pub llm_saved_preset_select_id: &'static str,
}

#[component]
pub(crate) fn SettingsAppearanceBlock(
    locale: RwSignal<Locale>,
    appearance_locale: RwSignal<Locale>,
    appearance_theme: RwSignal<String>,
    appearance_bg_decor: RwSignal<bool>,
    /// `<select id=…>`：设置页与弹窗可能同时挂载，须用不同 id。
    theme_select_id: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_language(locale.get())}</h3>
            <div class="settings-row">
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    class:active=move || appearance_locale.get() == Locale::ZhHans
                    on:click=move |_| appearance_locale.set(Locale::ZhHans)
                >
                    {move || i18n::settings_lang_zh(locale.get())}
                </button>
                <button
                    type="button"
                    class="btn btn-secondary btn-sm"
                    class:active=move || appearance_locale.get() == Locale::En
                    on:click=move |_| appearance_locale.set(Locale::En)
                >
                    {move || i18n::settings_lang_en(locale.get())}
                </button>
            </div>
        </div>
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_theme(locale.get())}</h3>
            <div class="settings-field">
                <label class="settings-field-label" for=theme_select_id>
                    {move || i18n::settings_label_theme_preset(locale.get())}
                </label>
                <select
                    id=theme_select_id
                    class="settings-select"
                    prop:value=move || appearance_theme.get()
                    on:change=move |ev| appearance_theme.set(event_target_value(&ev))
                >
                    {THEME_SLUGS.iter().copied().map(|slug| {
                        view! {
                            <option value=slug>
                                {move || i18n::settings_theme_preset_label(locale.get(), slug)}
                            </option>
                        }
                    }).collect_view()}
                </select>
            </div>
        </div>
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_bg(locale.get())}</h3>
            <label class="settings-checkbox-label">
                <input
                    type="checkbox"
                    prop:checked=move || appearance_bg_decor.get()
                    on:change=move |_| appearance_bg_decor.update(|v| *v = !*v)
                />
                <span>{move || i18n::settings_bg_glow(locale.get())}</span>
            </label>
        </div>
    }
}

#[component]
pub(crate) fn SettingsLlmBlock(bundle: SettingsLlmBlockBundle) -> impl IntoView {
    let SettingsLlmBlockBundle {
        locale,
        saved_model_presets,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
        execution_mode_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        clear_client_key_intent,
        llm_thinking_mode_select_id,
        llm_saved_preset_select_id,
    } = bundle;
    let main_drafts = MainLlmDraftSignals {
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
    };
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_llm(locale.get())}</h3>
            <LlmSavedPresetPicker
                locale
                saved_model_presets=saved_model_presets
                pick_target=LlmSavedPresetApplyTarget::Main(
                    main_drafts,
                    llm_api_key_draft,
                    llm_has_saved_key,
                    clear_client_key_intent,
                )
                select_id=llm_saved_preset_select_id
            />
            <LlmTemperatureField locale temperature_draft=llm_temperature_draft />
            <LlmContextTokensField locale llm_context_tokens_draft />
            <LlmThinkingModeField
                locale
                thinking_mode_draft=llm_thinking_mode_draft
                select_id=llm_thinking_mode_select_id
            />
            <OptionalLlmExecutionModeField locale execution_mode_draft=execution_mode_draft />
        </div>
    }
}

/// 设置页「执行器 LLM」区块信号。
#[derive(Clone, Copy)]
pub(crate) struct SettingsExecutorLlmBlockBundle {
    pub locale: RwSignal<Locale>,
    pub saved_model_presets: RwSignal<Vec<crate::api::SavedModelPreset>>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub clear_executor_key_intent: RwSignal<bool>,
    pub executor_saved_preset_select_id: &'static str,
}

#[component]
pub(crate) fn SettingsExecutorLlmBlock(bundle: SettingsExecutorLlmBlockBundle) -> impl IntoView {
    let SettingsExecutorLlmBlockBundle {
        locale,
        saved_model_presets,
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
        executor_llm_api_key_draft,
        executor_llm_has_saved_key,
        clear_executor_key_intent,
        executor_saved_preset_select_id,
    } = bundle;
    let exec_drafts = crate::api::ExecutorLlmDraftSignals {
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
    };
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_executor_llm(locale.get())}</h3>
            <LlmSavedPresetPicker
                locale
                saved_model_presets=saved_model_presets
                pick_target=LlmSavedPresetApplyTarget::Executor(
                    exec_drafts,
                    executor_llm_api_key_draft,
                    executor_llm_has_saved_key,
                    clear_executor_key_intent,
                )
                select_id=executor_saved_preset_select_id
            />
        </div>
    }
}

#[component]
pub(crate) fn SettingsToolsBlock(
    locale: RwSignal<Locale>,
    readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_tools_readonly_ttl_block_title(locale.get())}</h3>
            <label class="settings-checkbox-label">
                <input
                    type="checkbox"
                    prop:checked=move || readonly_tool_ttl_cache_follow_server.get()
                    on:change=move |_| {
                        readonly_tool_ttl_cache_follow_server.update(|v| *v = !*v);
                    }
                />
                <span>{move || i18n::settings_tools_readonly_ttl_cache_label(locale.get())}</span>
            </label>
        </div>
    }
}

#[component]
pub(crate) fn SettingsSessionBlock(
    locale: RwSignal<Locale>,
    status_data: RwSignal<Option<crate::api::StatusData>>,
    refresh_status: Arc<dyn Fn() + Send + Sync>,
    session_switch_feedback: RwSignal<Option<String>>,
    session_switch_busy: RwSignal<bool>,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_session_storage(locale.get())}</h3>
            <label class="settings-checkbox-label">
                <input
                    type="checkbox"
                    prop:disabled=move || {
                        session_switch_busy.get()
                            || !status_data
                                .get()
                                .map(|s| s.conversation_store_sqlite_path_configured)
                                .unwrap_or(false)
                    }
                    prop:checked=move || {
                        status_data
                            .get()
                            .map(|s| s.conversation_store_sqlite_active)
                            .unwrap_or(false)
                    }
                    on:change=move |ev: leptos::ev::Event| {
                        let checked = ev
                            .target()
                            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
                            .map(|el| el.checked())
                            .unwrap_or(false);
                        let loc = locale.get_untracked();
                        let refresh = Arc::clone(&refresh_status);
                        spawn_local(async move {
                            session_switch_busy.set(true);
                            session_switch_feedback.set(None);
                            match crate::api::post_session_conversation_store(checked, loc).await {
                                Ok(r) => {
                                    session_switch_feedback.set(Some(r.message));
                                    refresh();
                                }
                                Err(e) => session_switch_feedback.set(Some(e)),
                            }
                            session_switch_busy.set(false);
                        });
                    }
                />
                <span>{move || i18n::settings_session_sqlite_toggle_label(locale.get())}</span>
            </label>
            <Show when=move || {
                !status_data
                    .get()
                    .map(|s| s.conversation_store_sqlite_path_configured)
                    .unwrap_or(false)
            }>
                <p class="settings-intro">{move || {
                    i18n::settings_session_sqlite_unconfigured_hint(locale.get())
                }}</p>
            </Show>
            <Show when=move || session_switch_busy.get()>
                <p class="settings-intro">{move || i18n::settings_session_switch_busy(locale.get())}</p>
            </Show>
            <Show when=move || session_switch_feedback.get().is_some()>
                <p class="settings-save-feedback">{move || {
                    session_switch_feedback.get().unwrap_or_default()
                }}</p>
            </Show>
        </div>
    }
}

#[component]
pub(crate) fn SettingsShortcutsBlock(
    locale: RwSignal<Locale>,
    body_class: &'static str,
) -> impl IntoView {
    view! {
        <div class="settings-block">
            <h3 class="settings-block-title">{move || i18n::settings_block_shortcuts(locale.get())}</h3>
            <p class=body_class>{move || i18n::settings_shortcuts_body(locale.get())}</p>
        </div>
    }
}
