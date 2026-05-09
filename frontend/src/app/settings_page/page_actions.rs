//! 设置页「放弃更改 / 保存全部」副作用（从 `SettingsPageView` 拆出以降低 nloc 棘轮）。

use leptos::prelude::*;

use super::form_snapshot::{SettingsPageDraftSignals, form_current_untracked};
use crate::app::settings_commit::{CommitAllSettingsInput, commit_all_settings};
use crate::app::settings_form_state::SettingsDirtyBaselines;
use crate::i18n::{self, Locale};

/// `discard_to_baselines` 入参（单结构体以满足 `fn-param` 棘轮）。
#[derive(Clone, Copy)]
pub(crate) struct DiscardToBaselinesCtx {
    pub baselines: SettingsDirtyBaselines,
    pub drafts: SettingsPageDraftSignals,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
}

pub(crate) fn discard_to_baselines(ctx: DiscardToBaselinesCtx) {
    let DiscardToBaselinesCtx {
        baselines,
        drafts,
        llm_settings_feedback,
        executor_llm_settings_feedback,
    } = ctx;

    let (bl, bt, bbd) = baselines.appearance.get_value();
    drafts.appearance_locale.set(bl);
    drafts.appearance_theme.set(bt);
    drafts.appearance_bg_decor.set(bbd);

    let (bb, bp, bm, btemp, bct, btm, be, bh) = baselines.llm.get_value();
    drafts.llm_api_base_draft.set(bb);
    drafts.llm_api_base_preset_select.set(bp);
    drafts.llm_model_draft.set(bm);
    drafts.llm_temperature_draft.set(btemp);
    drafts.llm_context_tokens_draft.set(bct);
    drafts.llm_thinking_mode_draft.set(btm);
    drafts.execution_mode_draft.set(be);
    drafts.llm_has_saved_key.set(bh);
    drafts.llm_api_key_draft.set(String::new());

    let (eb, ep, em, eh) = baselines.executor.get_value();
    drafts.executor_llm_api_base_draft.set(eb);
    drafts.executor_llm_api_base_preset_select.set(ep);
    drafts.executor_llm_model_draft.set(em);
    drafts.executor_llm_has_saved_key.set(eh);
    drafts.executor_llm_api_key_draft.set(String::new());

    drafts
        .readonly_tool_ttl_cache_follow_server
        .set(baselines.readonly_tool_ttl_cache_follow_server.get_value());

    drafts
        .saved_model_presets
        .set(baselines.saved_model_presets.get_value());

    drafts.clear_client_key_intent.set(false);
    drafts.clear_executor_key_intent.set(false);
    llm_settings_feedback.set(None);
    executor_llm_settings_feedback.set(None);
}

/// `try_save_all_settings` 入参（单结构体以满足 `fn-param` 棘轮）。
#[derive(Clone, Copy)]
pub(crate) struct SaveAllSettingsCtx {
    pub dirty: Memo<bool>,
    pub appearance_locale: RwSignal<Locale>,
    pub locale: RwSignal<Locale>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub drafts: SettingsPageDraftSignals,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
    pub client_llm_storage_tick: RwSignal<u64>,
    pub baselines: SettingsDirtyBaselines,
}

pub(crate) fn try_save_all_settings(ctx: SaveAllSettingsCtx) {
    let SaveAllSettingsCtx {
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
    } = ctx;

    llm_settings_feedback.set(None);
    executor_llm_settings_feedback.set(None);
    if !dirty.get() {
        llm_settings_feedback.set(Some(
            i18n::settings_nothing_to_save(appearance_locale.get()).to_string(),
        ));
        return;
    }
    let ui_locale = appearance_locale.get();
    let saved_presets = drafts.saved_model_presets.get();
    match commit_all_settings(CommitAllSettingsInput {
        ui_locale,
        appearance_locale: drafts.appearance_locale.get(),
        appearance_theme: drafts.appearance_theme.get(),
        appearance_bg_decor: drafts.appearance_bg_decor.get(),
        locale,
        theme,
        bg_decor,
        client_base: drafts.llm_api_base_draft.get().as_str(),
        client_model: drafts.llm_model_draft.get().as_str(),
        client_temperature: drafts.llm_temperature_draft.get().as_str(),
        client_llm_context_tokens: drafts.llm_context_tokens_draft.get().as_str(),
        client_llm_thinking_mode: drafts.llm_thinking_mode_draft.get().as_str(),
        client_api_key_draft: drafts.llm_api_key_draft.get().as_str(),
        executor_base: drafts.executor_llm_api_base_draft.get().as_str(),
        executor_model: drafts.executor_llm_model_draft.get().as_str(),
        executor_api_key_draft: drafts.executor_llm_api_key_draft.get().as_str(),
        execution_mode: drafts.execution_mode_draft.get().as_str(),
        readonly_tool_ttl_cache_follow_server: drafts.readonly_tool_ttl_cache_follow_server.get(),
        clear_client_llm_key: drafts.clear_client_key_intent.get(),
        clear_executor_llm_key: drafts.clear_executor_key_intent.get(),
        llm_api_key_draft: drafts.llm_api_key_draft,
        llm_has_saved_key: drafts.llm_has_saved_key,
        executor_llm_api_key_draft: drafts.executor_llm_api_key_draft,
        executor_llm_has_saved_key: drafts.executor_llm_has_saved_key,
        client_llm_storage_tick,
        saved_model_presets: saved_presets.as_slice(),
    }) {
        Ok(()) => {
            baselines.refresh_from_current(&form_current_untracked(drafts));
            drafts.clear_client_key_intent.set(false);
            drafts.clear_executor_key_intent.set(false);
            llm_settings_feedback.set(Some(i18n::settings_save_all_ok(ui_locale).to_string()));
        }
        Err(e) => llm_settings_feedback.set(Some(e)),
    }
}
