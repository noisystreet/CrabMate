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
/// Web 设置中保存的模型上下文窗口 token 上限（`llm_context_tokens`，与后端 `[agent] llm_context_tokens` 一致）。
pub const CLIENT_LLM_CONTEXT_TOKENS_STORAGE_KEY: &str = "crabmate-client-llm-context-tokens";
/// Web 设置中保存的 **`thinking`** 策略覆盖（`llm_thinking_mode`：`server` / `on` / `off`，与 `POST /chat` 的 `client_llm` 字段一致）。
pub const CLIENT_LLM_THINKING_MODE_STORAGE_KEY: &str = "crabmate-client-llm-thinking-mode";
/// Web 设置中保存的云端 API 密钥（`client_llm.api_key`）；**仅存本机**。
pub const CLIENT_LLM_API_KEY_STORAGE_KEY: &str = "crabmate-client-llm-api-key";

/// Web 设置中保存的 Executor LLM 网关基址（`executor_llm.api_base`）。
pub const EXECUTOR_LLM_API_BASE_STORAGE_KEY: &str = "crabmate-executor-llm-api-base";
/// Web 设置中保存的 Executor 模型名（`executor_llm.model`）。
pub const EXECUTOR_LLM_MODEL_STORAGE_KEY: &str = "crabmate-executor-llm-model";
/// Web 设置中保存的 Executor 云端 API 密钥（`executor_llm.api_key`）；**仅存本机**。
pub const EXECUTOR_LLM_API_KEY_STORAGE_KEY: &str = "crabmate-executor-llm-api-key";
/// Web 设置中保存的执行模式覆盖（`rolling_planning` / `hierarchical`）。
pub const EXECUTION_MODE_STORAGE_KEY: &str = "crabmate-execution-mode";
/// 存在且为 **`1`** 时：浏览器在每条 `/chat/stream` 请求中附带 **`readonly_tool_ttl_cache_secs: 0`**，禁用只读类 **`run_command`** 短时缓存。
pub const DISABLE_READONLY_TOOL_TTL_CACHE_STORAGE_KEY: &str =
    "crabmate-disable-readonly-tool-ttl-cache";

/// `remove_when(trimmed)` 为真时 `remove_item`，否则 `set_item(trimmed)`。
fn persist_storage_trimmed<E: Fn() -> String>(
    st: &web_sys::Storage,
    key: &'static str,
    raw: &str,
    remove_when: impl FnOnce(&str) -> bool,
    on_write_err: E,
) -> Result<(), String> {
    let t = raw.trim();
    if remove_when(t) {
        let _ = st.remove_item(key);
    } else {
        st.set_item(key, t).map_err(|_| on_write_err())?;
    }
    Ok(())
}

/// 写入 `client_llm` 中不含密钥的字段（侧栏「本机模型」）。
fn persist_client_llm_non_secret_fields(
    st: &web_sys::Storage,
    api_base: &str,
    model: &str,
    temperature: &str,
    llm_context_tokens: &str,
    llm_thinking_mode: &str,
    loc: Locale,
) -> Result<(), String> {
    persist_storage_trimmed(
        st,
        CLIENT_LLM_API_BASE_STORAGE_KEY,
        api_base,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_api_base(loc).to_string(),
    )?;
    persist_storage_trimmed(
        st,
        CLIENT_LLM_MODEL_STORAGE_KEY,
        model,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_model(loc).to_string(),
    )?;
    persist_storage_trimmed(
        st,
        CLIENT_LLM_TEMPERATURE_STORAGE_KEY,
        temperature,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_model(loc).to_string(),
    )?;
    persist_storage_trimmed(
        st,
        CLIENT_LLM_CONTEXT_TOKENS_STORAGE_KEY,
        llm_context_tokens,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_model(loc).to_string(),
    )?;
    persist_storage_trimmed(
        st,
        CLIENT_LLM_THINKING_MODE_STORAGE_KEY,
        llm_thinking_mode,
        |t| t.is_empty() || t == "server",
        || crate::i18n::api_err_write_model(loc).to_string(),
    )?;
    Ok(())
}

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

/// 供设置弹窗加载：`api_base` / `model` / `temperature` / `llm_context_tokens` / `llm_thinking_mode` 的已存值（无则空串）。
pub fn load_client_llm_text_fields_from_storage() -> (String, String, String, String, String) {
    (
        storage_trimmed_item(CLIENT_LLM_API_BASE_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_MODEL_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_TEMPERATURE_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_CONTEXT_TOKENS_STORAGE_KEY).unwrap_or_default(),
        storage_trimmed_item(CLIENT_LLM_THINKING_MODE_STORAGE_KEY).unwrap_or_default(),
    )
}

/// 读取本地执行模式；无值时返回空串，表示跟随服务端默认。
pub fn load_execution_mode_from_storage() -> String {
    storage_trimmed_item(EXECUTION_MODE_STORAGE_KEY).unwrap_or_default()
}

/// 将模型相关设置写入 localStorage。`api_key` 为 `None` 时不改已存密钥；为 `Some("")` 可配合调用方在「清除」时 `remove_item`。
pub fn persist_client_llm_to_storage(
    api_base: &str,
    model: &str,
    temperature: &str,
    llm_context_tokens: &str,
    llm_thinking_mode: &str,
    api_key_update: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    persist_client_llm_non_secret_fields(
        &st,
        api_base,
        model,
        temperature,
        llm_context_tokens,
        llm_thinking_mode,
        loc,
    )?;
    if let Some(k) = api_key_update {
        persist_storage_trimmed(
            &st,
            CLIENT_LLM_API_KEY_STORAGE_KEY,
            k,
            |t| t.is_empty(),
            || crate::i18n::api_err_write_api_key(loc).to_string(),
        )?;
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
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_CONTEXT_TOKENS_STORAGE_KEY) {
        if let Ok(n) = v.parse::<u64>() {
            m.insert("llm_context_tokens".into(), Value::Number(n.into()));
        }
    }
    if let Some(v) = storage_trimmed_item(CLIENT_LLM_THINKING_MODE_STORAGE_KEY) {
        if v == "on" || v == "off" {
            m.insert("llm_thinking_mode".into(), Value::String(v));
        }
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
    persist_storage_trimmed(
        &st,
        EXECUTOR_LLM_API_BASE_STORAGE_KEY,
        api_base,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_api_base(loc).to_string(),
    )?;
    persist_storage_trimmed(
        &st,
        EXECUTOR_LLM_MODEL_STORAGE_KEY,
        model,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_model(loc).to_string(),
    )?;
    if let Some(k) = api_key_update {
        persist_storage_trimmed(
            &st,
            EXECUTOR_LLM_API_KEY_STORAGE_KEY,
            k,
            |t| t.is_empty(),
            || crate::i18n::api_err_write_api_key(loc).to_string(),
        )?;
    }
    Ok(())
}

/// 将执行模式写入 localStorage；空字符串表示清除覆盖并跟随服务端默认。
pub fn persist_execution_mode_to_storage(mode: &str, loc: Locale) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    persist_storage_trimmed(
        &st,
        EXECUTION_MODE_STORAGE_KEY,
        mode,
        |t| t.is_empty(),
        || crate::i18n::api_err_write_model(loc).to_string(),
    )?;
    Ok(())
}

/// 合并进 `/chat/stream` 请求体的 `execution_mode`（仅两种受支持值）。
pub fn execution_mode_for_chat_body() -> Option<String> {
    match storage_trimmed_item(EXECUTION_MODE_STORAGE_KEY).as_deref() {
        Some("rolling_planning") => Some("rolling_planning".to_string()),
        Some("hierarchical") => Some("hierarchical".to_string()),
        _ => None,
    }
}

/// **`true`**：跟随服务端 **`readonly_tool_ttl_cache_secs`**；**`false`**：每条聊天请求显式关闭该缓存。
pub fn load_readonly_tool_ttl_cache_follow_server_from_storage() -> bool {
    storage_trimmed_item(DISABLE_READONLY_TOOL_TTL_CACHE_STORAGE_KEY).as_deref() != Some("1")
}

pub fn persist_readonly_tool_ttl_cache_follow_server(
    follow_server: bool,
    loc: Locale,
) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    if follow_server {
        let _ = st.remove_item(DISABLE_READONLY_TOOL_TTL_CACHE_STORAGE_KEY);
    } else {
        st.set_item(DISABLE_READONLY_TOOL_TTL_CACHE_STORAGE_KEY, "1")
            .map_err(|_| crate::i18n::api_err_write_model(loc).to_string())?;
    }
    Ok(())
}

/// 写入 **`POST /chat/stream`** JSON：**`None`** 表示不覆盖；**`Some(0)`** 关闭该缓存。
pub fn readonly_tool_ttl_cache_secs_for_chat_body() -> Option<u64> {
    if load_readonly_tool_ttl_cache_follow_server_from_storage() {
        None
    } else {
        Some(0)
    }
}

pub fn clear_executor_llm_api_key_storage(loc: Locale) -> Result<(), String> {
    let st =
        local_storage().ok_or_else(|| crate::i18n::api_err_no_local_storage(loc).to_string())?;
    let _ = st.remove_item(EXECUTOR_LLM_API_KEY_STORAGE_KEY);
    Ok(())
}
