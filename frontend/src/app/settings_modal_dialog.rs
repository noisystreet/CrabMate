//! 设置弹窗 DOM（与 `settings_modal.rs` 中的状态/副作用拆分以降低圈复杂度）。

use std::sync::Arc;

use leptos::html::Div;
use leptos::prelude::*;

use crate::a11y::trap_tab_in_container;
use crate::i18n::{self, Locale};

use super::settings_sections::{
    SettingsAppearanceBlock, SettingsExecutorLlmBlock, SettingsLlmBlock, SettingsLlmBlockBundle,
    SettingsShortcutsBlock, SettingsToolsBlock,
};

/// 设置弹窗 `view!` 所需的信号与回调（单参数入口，满足 fn-param 棘轮）。
pub struct SettingsModalDialogInput {
    pub settings_modal: RwSignal<bool>,
    pub settings_dialog_ref: NodeRef<Div>,
    pub appearance_locale: RwSignal<Locale>,
    pub appearance_theme: RwSignal<String>,
    pub appearance_bg_decor: RwSignal<bool>,
    pub dirty: Memo<bool>,
    pub discard: Arc<dyn Fn() + Send + Sync>,
    pub close_modal: Arc<dyn Fn() + Send + Sync>,
    pub save_all: Arc<dyn Fn() + Send + Sync>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_thinking_mode_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub clear_client_key_intent: RwSignal<bool>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub clear_executor_key_intent: RwSignal<bool>,
    pub readonly_tool_ttl_cache_follow_server: RwSignal<bool>,
}

/// 弹窗可见时的整棵 DOM（与原先一致：内含 `Show`）。
pub fn settings_modal_dialog(input: SettingsModalDialogInput) -> impl IntoView {
    let SettingsModalDialogInput {
        settings_modal,
        settings_dialog_ref,
        appearance_locale,
        appearance_theme,
        appearance_bg_decor,
        dirty,
        discard,
        close_modal,
        save_all,
        llm_settings_feedback,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_thinking_mode_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        clear_client_key_intent,
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
        executor_llm_api_key_draft,
        executor_llm_has_saved_key,
        clear_executor_key_intent,
        readonly_tool_ttl_cache_follow_server,
    } = input;

    view! {
        <Show when=move || settings_modal.get()>
            <div class="modal-backdrop" on:click={
                let discard = discard.clone();
                let close_modal = close_modal.clone();
                move |_| {
                    if dirty.get() {
                        discard();
                    }
                    close_modal();
                }
            }>
                <div
                    class="modal"
                    node_ref=settings_dialog_ref
                    role="dialog"
                    aria-modal="true"
                    aria-labelledby="settings-modal-title"
                    tabindex="-1"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Tab" {
                            if let Some(el) = settings_dialog_ref.get() {
                                trap_tab_in_container(&ev, el.as_ref());
                            }
                        }
                    }
                >
                    <div class="modal-head">
                        <h2 class="modal-title" id="settings-modal-title">{move || i18n::settings_title(appearance_locale.get())}</h2>
                        <span class="modal-badge">{move || i18n::settings_badge_local(appearance_locale.get())}</span>
                        <Show when=move || dirty.get()>
                            <span class="settings-unsaved-pill">{move || i18n::settings_unsaved_badge(appearance_locale.get())}</span>
                        </Show>
                        <span class="modal-head-spacer"></span>
                        <button type="button" class="btn btn-secondary btn-sm" prop:disabled=move || !dirty.get() on:click={
                            let discard = discard.clone();
                            move |_| discard()
                        }>
                            {move || i18n::settings_discard_changes(appearance_locale.get())}
                        </button>
                        <button type="button" class="btn btn-primary btn-sm" prop:disabled=move || !dirty.get() on:click={
                            let save_all = save_all.clone();
                            move |_| save_all()
                        }>
                            {move || i18n::settings_save_all(appearance_locale.get())}
                        </button>
                        <button type="button" class="btn btn-ghost btn-sm" on:click={
                            let discard = discard.clone();
                            let close_modal = close_modal.clone();
                            move |_| {
                                if dirty.get() {
                                    discard();
                                }
                                close_modal();
                            }
                        }>
                            {move || i18n::settings_close(appearance_locale.get())}
                        </button>
                    </div>
                    <div class="modal-body">
                        <p class="modal-hint">{move || i18n::settings_intro(appearance_locale.get())}</p>
                        <Show when=move || llm_settings_feedback.get().is_some()>
                            <p class="settings-save-feedback settings-save-feedback-global">{move || {
                                llm_settings_feedback.get().unwrap_or_default()
                            }}</p>
                        </Show>
                        <SettingsAppearanceBlock
                            locale=appearance_locale
                            appearance_locale=appearance_locale
                            appearance_theme=appearance_theme
                            appearance_bg_decor=appearance_bg_decor
                            theme_select_id="settings-modal-appearance-theme"
                        />
                        <SettingsLlmBlock bundle=SettingsLlmBlockBundle {
                            locale: appearance_locale,
                            llm_api_base_draft,
                            llm_api_base_preset_select,
                            llm_model_draft,
                            llm_temperature_draft,
                            llm_context_tokens_draft,
                            llm_thinking_mode_draft,
                            execution_mode_draft: None,
                            llm_api_key_draft,
                            llm_has_saved_key,
                            clear_client_key_intent,
                            hint_class: "modal-hint settings-field-nested-hint",
                            llm_thinking_mode_select_id: "settings-modal-llm-thinking-mode",
                        } />
                        <SettingsExecutorLlmBlock
                            locale=appearance_locale
                            executor_llm_api_base_draft=executor_llm_api_base_draft
                            executor_llm_api_base_preset_select=executor_llm_api_base_preset_select
                            executor_llm_model_draft=executor_llm_model_draft
                            executor_llm_api_key_draft=executor_llm_api_key_draft
                            executor_llm_has_saved_key=executor_llm_has_saved_key
                            clear_executor_key_intent=clear_executor_key_intent
                            hint_class="modal-hint settings-field-nested-hint"
                        />
                        <SettingsToolsBlock
                            locale=appearance_locale
                            readonly_tool_ttl_cache_follow_server=readonly_tool_ttl_cache_follow_server
                        />
                        <SettingsShortcutsBlock
                            locale=appearance_locale
                            body_class="modal-hint"
                        />
                    </div>
                </div>
            </div>
        </Show>
    }
}
