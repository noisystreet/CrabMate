//! 已保存模型列表：本机持久化与删除确认（从 `mod` 拆出以降低单文件物理行数棘轮）。

use std::sync::Arc;

use leptos::prelude::*;

use crate::api::{SavedModelPreset, persist_saved_model_presets_to_storage};
use crate::i18n::{self, Locale};

pub(crate) fn window_confirm_delete_saved_model_preset(locale: Locale) -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    match win.confirm_with_message(i18n::settings_models_delete_confirm(locale)) {
        Ok(true) => true,
        _ => false,
    }
}

/// 将完整列表写入本机并刷新 dirty baseline；失败时写 `llm_settings_feedback`、**不**修改 `saved_model_presets`。
pub(crate) fn try_persist_saved_presets_with_feedback(
    next: Vec<SavedModelPreset>,
    loc: Locale,
    saved_model_presets: RwSignal<Vec<SavedModelPreset>>,
    sync_saved_presets_baseline: &Arc<dyn Fn() + Send + Sync>,
    llm_settings_feedback: RwSignal<Option<String>>,
) -> bool {
    match persist_saved_model_presets_to_storage(&next, loc) {
        Ok(()) => {
            saved_model_presets.set(next);
            sync_saved_presets_baseline();
            llm_settings_feedback.set(None);
            true
        }
        Err(_) => {
            llm_settings_feedback.set(Some(
                i18n::settings_models_presets_persist_failed(loc).to_string(),
            ));
            false
        }
    }
}
