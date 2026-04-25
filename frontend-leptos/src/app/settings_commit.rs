//! 设置「保存全部」：将外观草稿与 LLM 草稿一次性写入本机存储，并同步全局信号。

use leptos::prelude::*;

use crate::api::{
    clear_client_llm_api_key_storage, clear_executor_llm_api_key_storage,
    client_llm_storage_has_api_key, executor_llm_storage_has_api_key,
    persist_client_llm_to_storage, persist_executor_llm_to_storage,
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

/// 将语言 / 主题 / 背景与（可选）LLM 覆盖写入 `localStorage`，并更新全局 UI 信号。
///
/// - `api_key_update`：`None` 表示不改已存密钥；`Some(...)` 则按 `persist_*` 语义写入或清除。
#[allow(clippy::too_many_arguments)]
pub fn commit_all_settings(
    ui_locale: Locale,
    appearance_locale: Locale,
    appearance_theme: String,
    appearance_bg_decor: bool,
    locale: RwSignal<Locale>,
    theme: RwSignal<String>,
    bg_decor: RwSignal<bool>,
    client_base: &str,
    client_model: &str,
    client_temperature: &str,
    client_api_key_draft: &str,
    executor_base: &str,
    executor_model: &str,
    executor_api_key_draft: &str,
    clear_client_llm_key: bool,
    clear_executor_llm_key: bool,
    llm_api_key_draft: RwSignal<String>,
    llm_has_saved_key: RwSignal<bool>,
    executor_llm_api_key_draft: RwSignal<String>,
    executor_llm_has_saved_key: RwSignal<bool>,
    client_llm_storage_tick: RwSignal<u64>,
) -> Result<(), String> {
    if clear_client_llm_key {
        clear_client_llm_api_key_storage(ui_locale)?;
    }
    if clear_executor_llm_key {
        clear_executor_llm_api_key_storage(ui_locale)?;
    }

    let client_key_upd = if clear_client_llm_key {
        Some("")
    } else if client_api_key_draft.trim().is_empty() {
        None
    } else {
        Some(client_api_key_draft)
    };
    validate_temperature_override(client_temperature, ui_locale)?;
    persist_client_llm_to_storage(
        client_base,
        client_model,
        client_temperature,
        client_key_upd,
        ui_locale,
    )?;

    let executor_key_upd = if clear_executor_llm_key {
        Some("")
    } else if executor_api_key_draft.trim().is_empty() {
        None
    } else {
        Some(executor_api_key_draft)
    };
    persist_executor_llm_to_storage(executor_base, executor_model, executor_key_upd, ui_locale)?;

    locale.set(appearance_locale);
    store_locale_slug(appearance_locale.storage_slug());
    theme.set(appearance_theme);
    bg_decor.set(appearance_bg_decor);

    llm_api_key_draft.set(String::new());
    executor_llm_api_key_draft.set(String::new());
    llm_has_saved_key.set(client_llm_storage_has_api_key());
    executor_llm_has_saved_key.set(executor_llm_storage_has_api_key());
    client_llm_storage_tick.update(|n| *n = n.wrapping_add(1));

    Ok(())
}
