//! 侧栏「本机模型」：`client_llm.*` 在 `localStorage` 中的读写（不含 HTTP）。

use serde_json::Value;

use crate::i18n::Locale;

use super::browser::local_storage;

/// Web 设置中保存的 LLM 网关基址（`client_llm.api_base`）。
pub const CLIENT_LLM_API_BASE_STORAGE_KEY: &str = "crabmate-client-llm-api-base";
/// Web 设置中保存的模型名（`client_llm.model`）。
pub const CLIENT_LLM_MODEL_STORAGE_KEY: &str = "crabmate-client-llm-model";
/// Web 设置中保存的温度覆盖（`temperature`）。
pub const CLIENT_LLM_TEMPERATURE_STORAGE_KEY: &str = "crabmate-client-llm-temperature";
/// Web 设置中保存的云端 API 密钥（`client_llm.api_key`）；**仅存本机**。
pub const CLIENT_LLM_API_KEY_STORAGE_KEY: &str = "crabmate-client-llm-api-key";

/// Web 设置中保存的 Executor LLM 网关基址（`executor_llm.api_base`）。
pub const EXECUTOR_LLM_API_BASE_STORAGE_KEY: &str = "crabmate-executor-llm-api-base";
/// Web 设置中保存的 Executor 模型名（`executor_llm.model`）。
pub const EXECUTOR_LLM_MODEL_STORAGE_KEY: &str = "crabmate-executor-llm-model";
/// Web 设置中保存的 Executor 云端 API 密钥（`executor_llm.api_key`）；**仅存本机**。
pub const EXECUTOR_LLM_API_KEY_STORAGE_KEY: &str = "crabmate-executor-llm-api-key";

fn storage_trimmed_item(key: &str) -> Option<String> {
    let st = local_storage()?;
    let s = st.get_item(key).ok().flatten()?;
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 是否已在 localStorage 保存过 `client_llm.api_key`（不返回密钥内容）。
pub fn client_llm_storage_has_api_key() -> bool {
    storage_trimmed_item(CLIENT_LLM_API_KEY_STORAGE_KEY).is_some()
}

/// 供设置弹窗加载：`api_base` / `model` / `temperature` 的已存值（无则空串）。
pub fn load_client_llm_text_fields_from_storage() -> (String, String, String) {
    (
        storage_trimmed_item(CLIENT_LLM_API_BASE_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_MODEL_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_TEMPERATURE_STORAGE_KEY).unwrap_or_default(),
    )
}

/// 将模型相关设置写入 localStorage。`api_key` 为 `None` 时不改已存密钥；为 `Some("")` 可配合调用方在「清除」时 `remove_item`。
pub fn persist_client_llm_to_storage(
    api_base: &str,
    model: &str,
    temperature: &str,
    api_key_update: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let b = api_base.trim();
    let m = model.trim();
    if b.is_empty() {
        let _ = st.remove_item(CLIENT_LLM_API_BASE_STORAGE_KEY);
    } else {
        st.set_item(CLIENT_LLM_API_BASE_STORAGE_KEY, b)
            .map_err(|_| crate::i18n::api_err_write_api_base(loc).to_string())?;
    }
    if m.is_empty() {
        let _ = st.remove_item(CLIENT_LLM_MODEL_STORAGE_KEY);
    } else {
        st.set_item(CLIENT_LLM_MODEL_STORAGE_KEY, m)
            .map_err(|_| crate::i18n::api_err_write_model(loc).to_string())?;
    }
    let t = temperature.trim();
    if t.is_empty() {
        let _ = st.remove_item(CLIENT_LLM_TEMPERATURE_STORAGE_KEY);
    } else {
        st.set_item(CLIENT_LLM_TEMPERATURE_STORAGE_KEY, t)
            .map_err(|_| crate::i18n::api_err_write_model(loc).to_string())?;
    }
    if let Some(k) = api_key_update {
        let t = k.trim();
        if t.is_empty() {
            let _ = st.remove_item(CLIENT_LLM_API_KEY_STORAGE_KEY);
        } else {
            st.set_item(CLIENT_LLM_API_KEY_STORAGE_KEY, t)
                .map_err(|_| crate::i18n::api_err_write_api_key(loc).to_string())?;
        }
    }
    Ok(())
}

pub fn clear_client_llm_api_key_storage(loc: Locale) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let _ = st.remove_item(CLIENT_LLM_API_KEY_STORAGE_KEY);
    Ok(())
}

/// 合并进 `/chat/stream` 请求体的 `client_llm` 对象（省略未配置的字段）。
pub fn client_llm_json_for_chat_body() -> Option<Value> {
    let mut m = serde_json::Map::new();
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_API_BASE_STORAGE_KEY) {
        m.insert("api_base".into(), Value::String(v));
    }
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_MODEL_STORAGE_KEY) {
        m.insert("model".into(), Value::String(v));
    }
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_API_KEY_STORAGE_KEY) {
        m.insert("api_key".into(), Value::String(v));
    }
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

/// 从本机设置读取 `/chat/stream` 的 `temperature` 覆盖（0～2）。
pub fn chat_temperature_override_from_storage() -> Option<f64> {
    let raw = storage_trimmed_item(CLIENT_LLM_TEMPERATURE_STORAGE_KEY)?;
    let parsed = raw.parse::<f64>().ok()?;
    if !parsed.is_finite() || !(0.0..=2.0).contains(&parsed) {
        return None;
    }
    Some(parsed)
}

/// 合并进 `/chat/stream` 请求体的 `executor_llm` 对象（省略未配置的字段）。
pub fn executor_llm_json_for_chat_body() -> Option<Value> {
    let mut m = serde_json::Map::new();
    if let Some(v) = storage_trimmed_item(EXECUTOR_LLM_API_BASE_STORAGE_KEY) {
        m.insert("api_base".into(), Value::String(v));
    }
    if let Some(v) = storage_trimmed_item(EXECUTOR_LLM_MODEL_STORAGE_KEY) {
        m.insert("model".into(), Value::String(v));
    }
    if let Some(v) = storage_trimmed_item(EXECUTOR_LLM_API_KEY_STORAGE_KEY) {
        m.insert("api_key".into(), Value::String(v));
    }
    if m.is_empty() {
        None
    } else {
        Some(Value::Object(m))
    }
}

/// 是否已在 localStorage 保存过 `executor_llm.api_key`（不返回密钥内容）。
pub fn executor_llm_storage_has_api_key() -> bool {
    storage_trimmed_item(EXECUTOR_LLM_API_KEY_STORAGE_KEY).is_some()
}

/// 供设置弹窗加载：`api_base` / `model` 的已存值（无则空串）。
pub fn load_executor_llm_text_fields_from_storage() -> (String, String) {
    (
        storage_trimmed_item(EXECUTOR_LLM_API_BASE_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(EXECUTOR_LLM_MODEL_STORAGE_KEY).unwrap_or_default(),
    )
}

/// 将 Executor 模型相关设置写入 localStorage。`api_key` 为 `None` 时不改已存密钥；为 `Some("")` 可配合调用方在「清除」时 `remove_item`。
pub fn persist_executor_llm_to_storage(
    api_base: &str,
    model: &str,
    api_key_update: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let b = api_base.trim();
    let m = model.trim();
    if b.is_empty() {
        let _ = st.remove_item(EXECUTOR_LLM_API_BASE_STORAGE_KEY);
    } else {
        st.set_item(EXECUTOR_LLM_API_BASE_STORAGE_KEY, b)
            .map_err(|_| crate::i18n::api_err_write_api_base(loc).to_string())?;
    }
    if m.is_empty() {
        let _ = st.remove_item(EXECUTOR_LLM_MODEL_STORAGE_KEY);
    } else {
        st.set_item(EXECUTOR_LLM_MODEL_STORAGE_KEY, m)
            .map_err(|_| crate::i18n::api_err_write_model(loc).to_string())?;
    }
    if let Some(k) = api_key_update {
        let t = k.trim();
        if t.is_empty() {
            let _ = st.remove_item(EXECUTOR_LLM_API_KEY_STORAGE_KEY);
        } else {
            st.set_item(EXECUTOR_LLM_API_KEY_STORAGE_KEY, t)
                .map_err(|_| crate::i18n::api_err_write_api_key(loc).to_string())?;
        }
    }
    Ok(())
}

pub fn clear_executor_llm_api_key_storage(loc: Locale) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let _ = st.remove_item(EXECUTOR_LLM_API_KEY_STORAGE_KEY);
    Ok(())
}
