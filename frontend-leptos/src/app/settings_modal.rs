//! 设置：主题、背景、本机模型覆盖。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::sync::Arc;

use super::app_shell_ctx::SettingsModalSignals;
use super::settings_form_state::{is_settings_dirty, refresh_baselines};
use super::settings_modal_dialog::{SettingsModalDialogInput, settings_modal_dialog};
use crate::a11y::focus_first_in_modal_container;
use crate::app::settings_page::dom_preview::{
    apply_bg_decor_preview_to_dom, apply_theme_preview_to_dom,
};
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
    } = signals;

    let settings_dialog_ref = NodeRef::<Div>::new();

    let appearance_locale = RwSignal::new(locale.get_untracked());
    let appearance_theme = RwSignal::new(theme.get_untracked());
    let appearance_bg_decor = RwSignal::new(bg_decor.get_untracked());

    let baseline_appearance = StoredValue::new((
        locale.get_untracked(),
        theme.get_untracked(),
        bg_decor.get_untracked(),
    ));
    let baseline_llm = StoredValue::new((
        llm_api_base_draft.get_untracked(),
        llm_api_base_preset_select.get_untracked(),
        llm_model_draft.get_untracked(),
        llm_temperature_draft.get_untracked(),
        llm_context_tokens_draft.get_untracked(),
        llm_thinking_mode_draft.get_untracked(),
        execution_mode_draft.get_untracked(),
        llm_has_saved_key.get_untracked(),
    ));
    let baseline_executor = StoredValue::new((
        executor_llm_api_base_draft.get_untracked(),
        executor_llm_api_base_preset_select.get_untracked(),
        executor_llm_model_draft.get_untracked(),
        executor_llm_has_saved_key.get_untracked(),
    ));

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
    };

    let current_state_untracked = move || form_current_untracked(drafts);

    let capture_baselines_after_open = move || {
        spawn_local(async move {
            TimeoutFuture::new(0).await;
            if !settings_modal.get_untracked() {
                return;
            }
            appearance_locale.set(locale.get_untracked());
            appearance_theme.set(theme.get_untracked());
            appearance_bg_decor.set(bg_decor.get_untracked());
            refresh_baselines(
                baseline_appearance,
                baseline_llm,
                baseline_executor,
                &current_state_untracked(),
            );
            clear_client_key_intent.set(false);
            clear_executor_key_intent.set(false);
            llm_settings_feedback.set(None);
            executor_llm_settings_feedback.set(None);
        });
    };

    let discard = {
        let baseline_appearance = baseline_appearance;
        let baseline_llm = baseline_llm;
        let baseline_executor = baseline_executor;
        move || {
            discard_to_baselines(DiscardToBaselinesCtx {
                baseline_appearance,
                baseline_llm,
                baseline_executor,
                drafts,
                llm_settings_feedback,
                executor_llm_settings_feedback,
            });
        }
    };

    Effect::new({
        let settings_modal = settings_modal;
        move |_| {
            if !settings_modal.get() {
                discard();
                apply_theme_preview_to_dom(theme.get().as_str());
                apply_bg_decor_preview_to_dom(bg_decor.get());
                return;
            }
            capture_baselines_after_open();
        }
    });

    Effect::new(move |_| {
        if !settings_modal.get() {
            return;
        }
        apply_theme_preview_to_dom(appearance_theme.get().as_str());
        apply_bg_decor_preview_to_dom(appearance_bg_decor.get());
    });

    Effect::new({
        let settings_dialog_ref = settings_dialog_ref.clone();
        move |_| {
            if !settings_modal.get() {
                return;
            }
            let r = settings_dialog_ref.clone();
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if let Some(el) = r.get() {
                    focus_first_in_modal_container(el.as_ref());
                }
            });
        }
    });

    let dirty = Memo::new(move |_| {
        let current = form_current_tracked(drafts);
        is_settings_dirty(
            &current,
            &baseline_appearance.get_value(),
            &baseline_llm.get_value(),
            &baseline_executor.get_value(),
        )
    });

    let close_modal = {
        let settings_modal = settings_modal;
        move || {
            settings_modal.set(false);
        }
    };

    let save_all = {
        let dirty = dirty;
        let baseline_appearance = baseline_appearance;
        let baseline_llm = baseline_llm;
        let baseline_executor = baseline_executor;
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
                baseline_appearance,
                baseline_llm,
                baseline_executor,
            });
        }
    };

    let discard_rc: Arc<dyn Fn() + Send + Sync> = Arc::new(discard);
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
    })
}
