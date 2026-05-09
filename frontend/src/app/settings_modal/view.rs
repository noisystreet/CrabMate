//! 设置弹窗视图与业务闭包（壳级 `Effect` 见 [`super::effects`]）。

use leptos::html::Div;
use leptos::prelude::*;
use std::sync::Arc;

use super::effects::{
    SettingsModalWireBundle, wire_settings_modal_appearance_preview_effect,
    wire_settings_modal_focus_first_effect, wire_settings_modal_open_close_baseline_effect,
};
use crate::app::app_shell_ctx::SettingsModalSignals;
use crate::app::settings_form_state::SettingsDirtyBaselines;
use crate::app::settings_modal_dialog::{SettingsModalDialogInput, settings_modal_dialog};
use crate::app::settings_page::form_snapshot::{
    SettingsPageDraftSignals, form_current_tracked, form_current_untracked,
};
use crate::app::settings_page::page_actions::{
    DiscardToBaselinesCtx, SaveAllSettingsCtx, discard_to_baselines, try_save_all_settings,
};

pub fn settings_modal_view(signals: SettingsModalSignals) -> impl IntoView {
    let SettingsModalSignals {
        settings_modal,
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
    } = signals;

    let settings_dialog_ref = NodeRef::<Div>::new();

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

    let discard = {
        move || {
            discard_to_baselines(DiscardToBaselinesCtx {
                baselines,
                drafts,
                llm_settings_feedback,
                executor_llm_settings_feedback,
            });
        }
    };

    let bundle = SettingsModalWireBundle {
        settings_modal,
        locale,
        theme,
        bg_decor,
        appearance_locale,
        appearance_theme,
        appearance_bg_decor,
        baselines,
        drafts,
        clear_client_key_intent,
        clear_executor_key_intent,
        llm_settings_feedback,
        executor_llm_settings_feedback,
    };

    let discard_arc: Arc<dyn Fn() + Send + Sync> = Arc::new(discard);
    wire_settings_modal_open_close_baseline_effect(bundle, Arc::clone(&discard_arc));
    wire_settings_modal_appearance_preview_effect(bundle);
    wire_settings_modal_focus_first_effect(settings_modal, settings_dialog_ref.clone());

    let dirty = Memo::new(move |_| {
        let current = form_current_tracked(drafts);
        baselines.is_dirty(&current)
    });

    let close_modal = {
        let settings_modal = settings_modal;
        move || {
            settings_modal.set(false);
        }
    };

    let save_all = {
        let dirty = dirty;
        move || {
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

    let discard_rc: Arc<dyn Fn() + Send + Sync> = discard_arc;
    let close_modal_rc: Arc<dyn Fn() + Send + Sync> = Arc::new(close_modal);
    let save_all_rc: Arc<dyn Fn() + Send + Sync> = Arc::new(save_all);

    settings_modal_dialog(SettingsModalDialogInput {
        settings_modal,
        settings_dialog_ref,
        appearance_locale,
        appearance_theme,
        appearance_bg_decor,
        dirty,
        discard: discard_rc,
        close_modal: close_modal_rc,
        save_all: save_all_rc,
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
        saved_model_presets,
    })
}
