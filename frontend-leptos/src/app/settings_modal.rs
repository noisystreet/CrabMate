//! 设置：主题、背景、本机模型覆盖。

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;
use std::sync::Arc;

use crate::a11y::focus_first_in_modal_container;
use crate::app::settings_commit::{CommitAllSettingsInput, commit_all_settings};
use crate::i18n::{self};

use super::app_shell_ctx::SettingsModalSignals;
use super::settings_form_state::{SettingsFormCurrent, is_settings_dirty, refresh_baselines};
use super::settings_modal_dialog::{SettingsModalDialogInput, settings_modal_dialog};

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

    let current_state_untracked = move || SettingsFormCurrent {
        appearance_locale: appearance_locale.get_untracked(),
        appearance_theme: appearance_theme.get_untracked(),
        appearance_bg_decor: appearance_bg_decor.get_untracked(),
        llm_api_base_draft: llm_api_base_draft.get_untracked(),
        llm_api_base_preset_select: llm_api_base_preset_select.get_untracked(),
        llm_model_draft: llm_model_draft.get_untracked(),
        llm_temperature_draft: llm_temperature_draft.get_untracked(),
        llm_context_tokens_draft: llm_context_tokens_draft.get_untracked(),
        execution_mode_draft: execution_mode_draft.get_untracked(),
        llm_has_saved_key: llm_has_saved_key.get_untracked(),
        executor_llm_api_base_draft: executor_llm_api_base_draft.get_untracked(),
        executor_llm_api_base_preset_select: executor_llm_api_base_preset_select.get_untracked(),
        executor_llm_model_draft: executor_llm_model_draft.get_untracked(),
        executor_llm_has_saved_key: executor_llm_has_saved_key.get_untracked(),
        clear_client_key_intent: clear_client_key_intent.get_untracked(),
        clear_executor_key_intent: clear_executor_key_intent.get_untracked(),
        llm_api_key_draft: llm_api_key_draft.get_untracked(),
        executor_llm_api_key_draft: executor_llm_api_key_draft.get_untracked(),
    };

    fn apply_theme_preview_to_dom(theme: &str) {
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            let _ = root.set_attribute("data-theme", theme);
        }
    }

    fn apply_bg_decor_preview_to_dom(bg_decor: bool) {
        if let Some(doc) = web_sys::window().and_then(|w| w.document())
            && let Some(root) = doc.document_element()
        {
            if bg_decor {
                let _ = root.remove_attribute("data-bg-decor");
            } else {
                let _ = root.set_attribute("data-bg-decor", "plain");
            }
        }
    }

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
        move || {
            let (bl, bt, bbd) = baseline_appearance.get_value();
            appearance_locale.set(bl);
            appearance_theme.set(bt);
            appearance_bg_decor.set(bbd);

            let (bb, bp, bm, bt, bct, be, bh) = baseline_llm.get_value();
            llm_api_base_draft.set(bb);
            llm_api_base_preset_select.set(bp);
            llm_model_draft.set(bm);
            llm_temperature_draft.set(bt);
            llm_context_tokens_draft.set(bct);
            execution_mode_draft.set(be);
            llm_has_saved_key.set(bh);
            llm_api_key_draft.set(String::new());

            let (eb, ep, em, eh) = baseline_executor.get_value();
            executor_llm_api_base_draft.set(eb);
            executor_llm_api_base_preset_select.set(ep);
            executor_llm_model_draft.set(em);
            executor_llm_has_saved_key.set(eh);
            executor_llm_api_key_draft.set(String::new());

            clear_client_key_intent.set(false);
            clear_executor_key_intent.set(false);
            llm_settings_feedback.set(None);
            executor_llm_settings_feedback.set(None);
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
        let current = SettingsFormCurrent {
            appearance_locale: appearance_locale.get(),
            appearance_theme: appearance_theme.get(),
            appearance_bg_decor: appearance_bg_decor.get(),
            llm_api_base_draft: llm_api_base_draft.get(),
            llm_api_base_preset_select: llm_api_base_preset_select.get(),
            llm_model_draft: llm_model_draft.get(),
            llm_temperature_draft: llm_temperature_draft.get(),
            llm_context_tokens_draft: llm_context_tokens_draft.get(),
            execution_mode_draft: execution_mode_draft.get(),
            llm_has_saved_key: llm_has_saved_key.get(),
            executor_llm_api_base_draft: executor_llm_api_base_draft.get(),
            executor_llm_api_base_preset_select: executor_llm_api_base_preset_select.get(),
            executor_llm_model_draft: executor_llm_model_draft.get(),
            executor_llm_has_saved_key: executor_llm_has_saved_key.get(),
            clear_client_key_intent: clear_client_key_intent.get(),
            clear_executor_key_intent: clear_executor_key_intent.get(),
            llm_api_key_draft: llm_api_key_draft.get(),
            executor_llm_api_key_draft: executor_llm_api_key_draft.get(),
        };
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

    let save_all = move || {
        llm_settings_feedback.set(None);
        executor_llm_settings_feedback.set(None);
        if !dirty.get() {
            llm_settings_feedback.set(Some(
                i18n::settings_nothing_to_save(appearance_locale.get()).to_string(),
            ));
            return;
        }
        let ui_locale = appearance_locale.get();
        match commit_all_settings(CommitAllSettingsInput {
            ui_locale,
            appearance_locale: appearance_locale.get(),
            appearance_theme: appearance_theme.get(),
            appearance_bg_decor: appearance_bg_decor.get(),
            locale,
            theme,
            bg_decor,
            client_base: llm_api_base_draft.get().as_str(),
            client_model: llm_model_draft.get().as_str(),
            client_temperature: llm_temperature_draft.get().as_str(),
            client_llm_context_tokens: llm_context_tokens_draft.get().as_str(),
            client_api_key_draft: llm_api_key_draft.get().as_str(),
            executor_base: executor_llm_api_base_draft.get().as_str(),
            executor_model: executor_llm_model_draft.get().as_str(),
            executor_api_key_draft: executor_llm_api_key_draft.get().as_str(),
            execution_mode: execution_mode_draft.get().as_str(),
            clear_client_llm_key: clear_client_key_intent.get(),
            clear_executor_llm_key: clear_executor_key_intent.get(),
            llm_api_key_draft,
            llm_has_saved_key,
            executor_llm_api_key_draft,
            executor_llm_has_saved_key,
            client_llm_storage_tick,
        }) {
            Ok(()) => {
                refresh_baselines(
                    baseline_appearance,
                    baseline_llm,
                    baseline_executor,
                    &current_state_untracked(),
                );
                clear_client_key_intent.set(false);
                clear_executor_key_intent.set(false);
                llm_settings_feedback.set(Some(i18n::settings_save_all_ok(ui_locale).to_string()));
            }
            Err(e) => llm_settings_feedback.set(Some(e)),
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
