//! 设置弹窗/页面打开时，用 **`localStorage`** 与 **`/status`** 快照填充 LLM 草稿（高频订阅 `settings_modal` / `settings_page`，**不**订阅 `sessions`）。

use leptos::prelude::*;

use crate::api::{
    StatusData, client_llm_storage_has_api_key, load_client_llm_text_fields_from_storage,
    load_execution_mode_from_storage, load_saved_model_presets_from_storage,
};

use crate::app::app_signals::LLMSettingsSignals;
use crate::app::status_tasks_state::StatusTasksSignals;

/// 刻意 **不** 把 `status_data` 的变更订阅进设置填充 `Effect`：仅在设置 UI 打开时读取快照，
/// 避免 `/status` 轮询导致草稿被反复重置。
fn status_snapshot_for_llm_drafts(status_tasks: &StatusTasksSignals) -> Option<StatusData> {
    status_tasks.status_data.get_untracked()
}

/// 打开设置弹窗或设置页面时，用 **`localStorage`** 与 **`/status`** 快照填充 LLM 草稿区（缩短 [`wire_settings_modal_llm_drafts_on_open`] 形参列表）。
#[derive(Clone, Copy)]
pub struct WireSettingsModalLlmDraftsSignals {
    pub settings_modal: RwSignal<bool>,
    pub settings_page: RwSignal<bool>,
    pub status_tasks: StatusTasksSignals,
    pub llm: LLMSettingsSignals,
}

/// 打开设置弹窗或设置页面时，用 **`localStorage`** 与 **`/status`** 快照填充 LLM 草稿区。
pub fn wire_settings_modal_llm_drafts_on_open(s: WireSettingsModalLlmDraftsSignals) {
    let WireSettingsModalLlmDraftsSignals {
        settings_modal,
        settings_page,
        status_tasks,
        llm:
            LLMSettingsSignals {
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
                saved_model_presets,
                ..
            },
    } = s;
    Effect::new(move |_| {
        if !settings_modal.get() && !settings_page.get() {
            return;
        }
        let (stored_base, stored_model, stored_temperature, stored_ctx_tokens, stored_thinking) =
            load_client_llm_text_fields_from_storage();
        let sd = status_snapshot_for_llm_drafts(&status_tasks);
        let base = if stored_base.trim().is_empty() {
            sd.as_ref().map(|d| d.api_base.clone()).unwrap_or_default()
        } else {
            stored_base
        };
        let model = if stored_model.trim().is_empty() {
            sd.as_ref().map(|d| d.model.clone()).unwrap_or_default()
        } else {
            stored_model
        };
        llm_api_base_draft.set(base.clone());
        llm_api_base_preset_select.set(
            crate::client_llm_presets::api_base_select_value_for_draft(base.as_str()).to_string(),
        );
        llm_model_draft.set(model);
        llm_temperature_draft.set(stored_temperature);
        let ctx_tokens = if stored_ctx_tokens.trim().is_empty() {
            sd.as_ref()
                .map(|d| d.llm_context_tokens.to_string())
                .unwrap_or_default()
        } else {
            stored_ctx_tokens
        };
        llm_context_tokens_draft.set(ctx_tokens);
        let thinking = stored_thinking.trim();
        if thinking == "on" || thinking == "off" {
            llm_thinking_mode_draft.set(thinking.to_string());
        } else {
            llm_thinking_mode_draft.set("server".to_string());
        }
        llm_api_key_draft.set(String::new());
        llm_has_saved_key.set(client_llm_storage_has_api_key());
        llm_settings_feedback.set(None);

        let (executor_stored_base, executor_stored_model) =
            crate::api::load_executor_llm_text_fields_from_storage();
        let executor_base = if executor_stored_base.trim().is_empty() {
            sd.as_ref()
                .map(|d| d.executor_api_base.clone())
                .unwrap_or_default()
        } else {
            executor_stored_base
        };
        let executor_model = if executor_stored_model.trim().is_empty() {
            sd.as_ref()
                .map(|d| d.executor_model.clone())
                .unwrap_or_default()
        } else {
            executor_stored_model
        };
        executor_llm_api_base_draft.set(executor_base.clone());
        executor_llm_api_base_preset_select.set(
            crate::client_llm_presets::api_base_select_value_for_draft(executor_base.as_str())
                .to_string(),
        );
        executor_llm_model_draft.set(executor_model);
        executor_llm_api_key_draft.set(String::new());
        executor_llm_has_saved_key.set(crate::api::executor_llm_storage_has_api_key());
        executor_llm_settings_feedback.set(None);
        let mode = load_execution_mode_from_storage();
        if mode == "rolling_planning" || mode == "hierarchical" {
            execution_mode_draft.set(mode);
        } else {
            let server_mode = sd
                .as_ref()
                .map(|d| d.planner_executor_mode.trim().to_string())
                .unwrap_or_default();
            if server_mode == "hierarchical" {
                execution_mode_draft.set("hierarchical".to_string());
            } else {
                execution_mode_draft.set("rolling_planning".to_string());
            }
        }
        saved_model_presets.set(load_saved_model_presets_from_storage());
    });
}
