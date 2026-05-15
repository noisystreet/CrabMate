//! 设置表单：当前草稿快照、与 **已提交 baseline** 的 dirty 比较、保存后刷新 baseline。
//! 三元 baseline 句柄见 [`SettingsDirtyBaselines`]。

use leptos::prelude::*;

use crate::api::SavedModelPreset;
use crate::i18n::Locale;

pub(crate) type AppearanceBaseline = (Locale, String, bool);
pub(crate) type LlmBaseline = (String, String, String, String, String, String, String, bool);
pub(crate) type ExecutorBaseline = (String, String, String, bool);

/// 设置弹窗 / 设置页「已提交快照」：`StoredValue` 三元组打包，避免在 dirty / 放弃 / 保存成功路径上重复传三个句柄。
#[derive(Clone, Copy)]
pub(crate) struct SettingsDirtyBaselines {
    pub appearance: StoredValue<AppearanceBaseline>,
    pub llm: StoredValue<LlmBaseline>,
    pub executor: StoredValue<ExecutorBaseline>,
    pub saved_model_presets: StoredValue<Vec<SavedModelPreset>>,
    pub readonly_tool_ttl_cache_follow_server: StoredValue<bool>,
}

impl SettingsDirtyBaselines {
    #[must_use]
    pub(crate) fn from_form_current(current: &SettingsFormCurrent) -> Self {
        Self {
            appearance: StoredValue::new((
                current.appearance_locale,
                current.appearance_theme.clone(),
                current.appearance_bg_decor,
            )),
            llm: StoredValue::new((
                current.llm_api_base_draft.clone(),
                current.llm_api_base_preset_select.clone(),
                current.llm_model_draft.clone(),
                current.llm_temperature_draft.clone(),
                current.llm_context_tokens_draft.clone(),
                current.llm_thinking_mode_draft.clone(),
                current.execution_mode_draft.clone(),
                current.llm_has_saved_key,
            )),
            executor: StoredValue::new((
                current.executor_llm_api_base_draft.clone(),
                current.executor_llm_api_base_preset_select.clone(),
                current.executor_llm_model_draft.clone(),
                current.executor_llm_has_saved_key,
            )),
            readonly_tool_ttl_cache_follow_server: StoredValue::new(
                current.readonly_tool_ttl_cache_follow_server,
            ),
            saved_model_presets: StoredValue::new(current.saved_model_presets.clone()),
        }
    }

    pub(crate) fn refresh_from_current(&self, current: &SettingsFormCurrent) {
        refresh_baselines(
            self.appearance,
            self.llm,
            self.executor,
            self.saved_model_presets,
            self.readonly_tool_ttl_cache_follow_server,
            current,
        );
    }

    pub(crate) fn is_dirty(&self, current: &SettingsFormCurrent) -> bool {
        is_settings_dirty(
            current,
            &self.appearance.get_value(),
            &self.llm.get_value(),
            &self.executor.get_value(),
            &self.saved_model_presets.get_value(),
            self.readonly_tool_ttl_cache_follow_server.get_value(),
        )
    }
}

/// 将壳层已生效外观信号复制到设置草稿（**仅** `get_untracked` 读壳层，避免额外汇入 `Effect` 订阅）。
pub(crate) fn sync_appearance_drafts_from_shell(
    shell_locale: RwSignal<Locale>,
    shell_theme: RwSignal<String>,
    shell_bg_decor: RwSignal<bool>,
    appearance_locale: RwSignal<Locale>,
    appearance_theme: RwSignal<String>,
    appearance_bg_decor: RwSignal<bool>,
) {
    appearance_locale.set(shell_locale.get_untracked());
    appearance_theme.set(shell_theme.get_untracked());
    appearance_bg_decor.set(shell_bg_decor.get_untracked());
}

#[derive(Clone)]
pub(crate) struct SettingsFormCurrent {
    pub appearance_locale: Locale,
    pub appearance_theme: String,
    pub appearance_bg_decor: bool,
    pub llm_api_base_draft: String,
    pub llm_api_base_preset_select: String,
    pub llm_model_draft: String,
    pub llm_temperature_draft: String,
    pub llm_context_tokens_draft: String,
    pub llm_thinking_mode_draft: String,
    pub execution_mode_draft: String,
    pub llm_has_saved_key: bool,
    pub executor_llm_api_base_draft: String,
    pub executor_llm_api_base_preset_select: String,
    pub executor_llm_model_draft: String,
    pub executor_llm_has_saved_key: bool,
    pub clear_client_key_intent: bool,
    pub clear_executor_key_intent: bool,
    pub llm_api_key_draft: String,
    pub executor_llm_api_key_draft: String,
    pub saved_model_presets: Vec<SavedModelPreset>,
    pub readonly_tool_ttl_cache_follow_server: bool,
}

fn appearance_dirty(current: &SettingsFormCurrent, baseline: &AppearanceBaseline) -> bool {
    let (bl, bt, bbd) = baseline;
    current.appearance_locale != *bl
        || current.appearance_theme != *bt
        || current.appearance_bg_decor != *bbd
}

fn pending_key_drafts_or_clear_intent_dirty(current: &SettingsFormCurrent) -> bool {
    current.clear_client_key_intent
        || current.clear_executor_key_intent
        || !current.llm_api_key_draft.trim().is_empty()
        || !current.executor_llm_api_key_draft.trim().is_empty()
}

fn llm_baseline_dirty(current: &SettingsFormCurrent, baseline: &LlmBaseline) -> bool {
    let (bb, bp, bm, bt, bct, btm, be, bh) = baseline;
    current.llm_api_base_draft != *bb
        || current.llm_api_base_preset_select != *bp
        || current.llm_model_draft != *bm
        || current.llm_temperature_draft != *bt
        || current.llm_context_tokens_draft != *bct
        || current.llm_thinking_mode_draft != *btm
        || current.execution_mode_draft != *be
        || current.llm_has_saved_key != *bh
}

fn executor_baseline_dirty(current: &SettingsFormCurrent, baseline: &ExecutorBaseline) -> bool {
    let (eb, ep, em, eh) = baseline;
    current.executor_llm_api_base_draft != *eb
        || current.executor_llm_api_base_preset_select != *ep
        || current.executor_llm_model_draft != *em
        || current.executor_llm_has_saved_key != *eh
}

fn saved_model_presets_dirty(current: &SettingsFormCurrent, baseline: &[SavedModelPreset]) -> bool {
    current.saved_model_presets.as_slice() != baseline
}

pub(crate) fn is_settings_dirty(
    current: &SettingsFormCurrent,
    baseline_appearance: &AppearanceBaseline,
    baseline_llm: &LlmBaseline,
    baseline_executor: &ExecutorBaseline,
    baseline_saved_model_presets: &[SavedModelPreset],
    baseline_readonly_tool_ttl_cache_follow_server: bool,
) -> bool {
    appearance_dirty(current, baseline_appearance)
        || pending_key_drafts_or_clear_intent_dirty(current)
        || llm_baseline_dirty(current, baseline_llm)
        || executor_baseline_dirty(current, baseline_executor)
        || saved_model_presets_dirty(current, baseline_saved_model_presets)
        || current.readonly_tool_ttl_cache_follow_server
            != baseline_readonly_tool_ttl_cache_follow_server
}

pub(crate) fn refresh_baselines(
    baseline_appearance: StoredValue<AppearanceBaseline>,
    baseline_llm: StoredValue<LlmBaseline>,
    baseline_executor: StoredValue<ExecutorBaseline>,
    baseline_saved_model_presets: StoredValue<Vec<SavedModelPreset>>,
    baseline_readonly_tool_ttl_cache_follow_server: StoredValue<bool>,
    current: &SettingsFormCurrent,
) {
    let _ = baseline_appearance.try_update_value(|v| {
        *v = (
            current.appearance_locale,
            current.appearance_theme.clone(),
            current.appearance_bg_decor,
        );
    });
    let _ = baseline_llm.try_update_value(|v| {
        *v = (
            current.llm_api_base_draft.clone(),
            current.llm_api_base_preset_select.clone(),
            current.llm_model_draft.clone(),
            current.llm_temperature_draft.clone(),
            current.llm_context_tokens_draft.clone(),
            current.llm_thinking_mode_draft.clone(),
            current.execution_mode_draft.clone(),
            current.llm_has_saved_key,
        );
    });
    let _ = baseline_executor.try_update_value(|v| {
        *v = (
            current.executor_llm_api_base_draft.clone(),
            current.executor_llm_api_base_preset_select.clone(),
            current.executor_llm_model_draft.clone(),
            current.executor_llm_has_saved_key,
        );
    });
    let _ = baseline_saved_model_presets.try_update_value(|v| {
        *v = current.saved_model_presets.clone();
    });
    let _ = baseline_readonly_tool_ttl_cache_follow_server.try_update_value(|v| {
        *v = current.readonly_tool_ttl_cache_follow_server;
    });
}

/// 设置草稿相对 baseline 的 UI 生命期相（不含网络「保存中」态；保存中仍以既有 `RwSignal` 为准）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsFormUiPhase {
    /// 草稿与已提交 baseline 一致且无未决密钥草稿/清除意图
    BaselineClean,
    /// 存在未保存变更或待提交 API 密钥草稿
    Dirty,
}

#[must_use]
pub(crate) fn derive_settings_form_ui_phase(
    current: &SettingsFormCurrent,
    baselines: &SettingsDirtyBaselines,
) -> SettingsFormUiPhase {
    if baselines.is_dirty(current) {
        SettingsFormUiPhase::Dirty
    } else {
        SettingsFormUiPhase::BaselineClean
    }
}

#[cfg(test)]
mod settings_form_ui_phase_tests {
    use super::{
        SettingsDirtyBaselines, SettingsFormCurrent, SettingsFormUiPhase,
        derive_settings_form_ui_phase,
    };

    fn sample_current(llm_model_draft: &str) -> SettingsFormCurrent {
        SettingsFormCurrent {
            appearance_locale: crate::i18n::Locale::ZhHans,
            appearance_theme: "dark".into(),
            appearance_bg_decor: false,
            llm_api_base_draft: String::new(),
            llm_api_base_preset_select: "server".into(),
            llm_model_draft: llm_model_draft.into(),
            llm_temperature_draft: String::new(),
            llm_context_tokens_draft: String::new(),
            llm_thinking_mode_draft: "server".into(),
            execution_mode_draft: "rolling_planning".into(),
            llm_has_saved_key: false,
            executor_llm_api_base_draft: String::new(),
            executor_llm_api_base_preset_select: "server".into(),
            executor_llm_model_draft: String::new(),
            executor_llm_has_saved_key: false,
            clear_client_key_intent: false,
            clear_executor_key_intent: false,
            llm_api_key_draft: String::new(),
            executor_llm_api_key_draft: String::new(),
            saved_model_presets: vec![],
            readonly_tool_ttl_cache_follow_server: true,
        }
    }

    #[test]
    fn derive_phase_matches_dirty_semantics() {
        let baselines = SettingsDirtyBaselines::from_form_current(&sample_current(""));
        assert_eq!(
            derive_settings_form_ui_phase(&sample_current(""), &baselines),
            SettingsFormUiPhase::BaselineClean
        );
        assert_eq!(
            derive_settings_form_ui_phase(&sample_current("m"), &baselines),
            SettingsFormUiPhase::Dirty
        );
    }
}
