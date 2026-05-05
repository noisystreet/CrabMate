//! 设置「保存全部」：将外观草稿与 LLM 草稿一次性写入本机存储，并同步全局信号。

use leptos::prelude::*;

use crate::api::{
    clear_client_llm_api_key_storage, clear_executor_llm_api_key_storage,
    client_llm_storage_has_api_key, executor_llm_storage_has_api_key,
    persist_client_llm_to_storage, persist_execution_mode_to_storage,
    persist_executor_llm_to_storage,
};
use crate::i18n::{Locale, store_locale_slug};

fn validate_temperature_override(raw: &str, loc: Locale) -> Result<(), String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(());
    }
    let parsed = t
        .parse::<f64>()
        .map_err(|_| crate::i18n::settings_err_temperature_invalid(loc).to_string())?;
    if !parsed.is_finite() || !(0.0..=2.0).contains(&parsed) {
        return Err(crate::i18n::settings_err_temperature_range(loc).to_string());
    }
    Ok(())
}

fn validate_llm_context_tokens_override(raw: &str, loc: Locale) -> Result<(), String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(());
    }
    let parsed = t
        .parse::<u64>()
        .map_err(|_| crate::i18n::settings_err_context_tokens_invalid(loc).to_string())?;
    if parsed > 10_000_000 {
        return Err(crate::i18n::settings_err_context_tokens_range(loc).to_string());
    }
    Ok(())
}

fn validate_llm_thinking_mode_override(raw: &str, loc: Locale) -> Result<(), String> {
    let t = raw.trim();
    if t.is_empty() || t == "server" || t == "on" || t == "off" {
        return Ok(());
    }
    Err(crate::i18n::settings_err_thinking_mode_invalid(loc).to_string())
}

/// 一次「保存全部设置」所需的表单快照与 UI 信号（避免长参数列表）。
pub struct CommitAllSettingsInput<'a> {
    pub ui_locale: Locale,
    pub appearance_locale: Locale,
    pub appearance_theme: String,
    pub appearance_bg_decor: bool,
    pub locale: RwSignal<Locale>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub client_base: &'a str,
    pub client_model: &'a str,
    pub client_temperature: &'a str,
    pub client_llm_context_tokens: &'a str,
    pub client_llm_thinking_mode: &'a str,
    pub client_api_key_draft: &'a str,
    pub executor_base: &'a str,
    pub executor_model: &'a str,
    pub executor_api_key_draft: &'a str,
    pub execution_mode: &'a str,
    pub clear_client_llm_key: bool,
    pub clear_executor_llm_key: bool,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub client_llm_storage_tick: RwSignal<u64>,
}

/// 将语言 / 主题 / 背景与（可选）LLM 覆盖写入 `localStorage`，并更新全局 UI 信号。
///
/// - 密钥草稿：`clear_*` 为真或草稿非空时按 `persist_*` 语义写入或清除。
pub fn commit_all_settings(p: CommitAllSettingsInput<'_>) -> Result<(), String> {
    if p.clear_client_llm_key {
        clear_client_llm_api_key_storage(p.ui_locale)?;
    }
    if p.clear_executor_llm_key {
        clear_executor_llm_api_key_storage(p.ui_locale)?;
    }

    let client_key_upd = if p.clear_client_llm_key {
        Some("")
    } else if p.client_api_key_draft.trim().is_empty() {
        None
    } else {
        Some(p.client_api_key_draft)
    };
    validate_temperature_override(p.client_temperature, p.ui_locale)?;
    validate_llm_context_tokens_override(p.client_llm_context_tokens, p.ui_locale)?;
    validate_llm_thinking_mode_override(p.client_llm_thinking_mode, p.ui_locale)?;
    persist_client_llm_to_storage(
        p.client_base,
        p.client_model,
        p.client_temperature,
        p.client_llm_context_tokens,
        p.client_llm_thinking_mode,
        client_key_upd,
        p.ui_locale,
    )?;

    let executor_key_upd = if p.clear_executor_llm_key {
        Some("")
    } else if p.executor_api_key_draft.trim().is_empty() {
        None
    } else {
        Some(p.executor_api_key_draft)
    };
    persist_executor_llm_to_storage(
        p.executor_base,
        p.executor_model,
        executor_key_upd,
        p.ui_locale,
    )?;
    persist_execution_mode_to_storage(p.execution_mode, p.ui_locale)?;

    p.locale.set(p.appearance_locale);
    store_locale_slug(p.appearance_locale.storage_slug());
    p.theme.set(p.appearance_theme);
    p.bg_decor.set(p.appearance_bg_decor);

    p.llm_api_key_draft.set(String::new());
    p.executor_llm_api_key_draft.set(String::new());
    p.llm_has_saved_key.set(client_llm_storage_has_api_key());
    p.executor_llm_has_saved_key
        .set(executor_llm_storage_has_api_key());
    p.client_llm_storage_tick.update(|n| *n = n.wrapping_add(1));

    Ok(())
}
