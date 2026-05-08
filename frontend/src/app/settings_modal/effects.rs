//! 设置弹窗壳级 `Effect`（从 `view` 拆出以降低 `settings_modal_view` nloc）。

use std::sync::Arc;

use gloo_timers::future::TimeoutFuture;
use leptos::html::Div;
use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::a11y::focus_first_in_modal_container;
use crate::app::settings_form_state::{SettingsDirtyBaselines, sync_appearance_drafts_from_shell};
use crate::app::settings_page::dom_preview::{
    apply_bg_decor_preview_to_dom, apply_theme_preview_to_dom,
};
use crate::app::settings_page::form_snapshot::{SettingsPageDraftSignals, form_current_untracked};
use crate::i18n::Locale;

/// 打开/关闭弹窗、baseline 捕获与壳层主题预览重置。
#[derive(Clone, Copy)]
pub(super) struct SettingsModalWireBundle {
    pub settings_modal: RwSignal<bool>,
    pub locale: RwSignal<Locale>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub appearance_locale: RwSignal<Locale>,
    pub appearance_theme: RwSignal<String>,
    pub appearance_bg_decor: RwSignal<bool>,
    pub baselines: SettingsDirtyBaselines,
    pub drafts: SettingsPageDraftSignals,
    pub clear_client_key_intent: RwSignal<bool>,
    pub clear_executor_key_intent: RwSignal<bool>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
}

pub(super) fn wire_settings_modal_open_close_baseline_effect(
    b: SettingsModalWireBundle,
    discard: Arc<dyn Fn() + Send + Sync>,
) {
    Effect::new({
        let discard = Arc::clone(&discard);
        move |_| {
            if !b.settings_modal.get() {
                discard();
                apply_theme_preview_to_dom(b.theme.get().as_str());
                apply_bg_decor_preview_to_dom(b.bg_decor.get());
                return;
            }
            spawn_local(async move {
                TimeoutFuture::new(0).await;
                if !b.settings_modal.get_untracked() {
                    return;
                }
                sync_appearance_drafts_from_shell(
                    b.locale,
                    b.theme,
                    b.bg_decor,
                    b.appearance_locale,
                    b.appearance_theme,
                    b.appearance_bg_decor,
                );
                b.baselines
                    .refresh_from_current(&form_current_untracked(b.drafts));
                b.clear_client_key_intent.set(false);
                b.clear_executor_key_intent.set(false);
                b.llm_settings_feedback.set(None);
                b.executor_llm_settings_feedback.set(None);
            });
        }
    });
}

pub(super) fn wire_settings_modal_appearance_preview_effect(b: SettingsModalWireBundle) {
    Effect::new(move |_| {
        if !b.settings_modal.get() {
            return;
        }
        apply_theme_preview_to_dom(b.appearance_theme.get().as_str());
        apply_bg_decor_preview_to_dom(b.appearance_bg_decor.get());
    });
}

pub(super) fn wire_settings_modal_focus_first_effect(
    settings_modal: RwSignal<bool>,
    settings_dialog_ref: NodeRef<Div>,
) {
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
}
