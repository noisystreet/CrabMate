//! 侧栏「本机模型」：进程内缓存 + **`/user-data/llm-overrides`** / **`secrets/*`**。

use serde_json::Value;

use crate::i18n::Locale;

use super::client_llm_cache::{self, with_mem, with_mem_mut};
use super::user_data::{
    LlmOverridesDto, put_llm_overrides, put_secret_client_llm, put_secret_executor_llm,
};

pub async fn hydrate_client_llm_from_server(loc: Locale) {
    client_llm_cache::hydrate_from_server(loc).await;
}

fn opt_trim(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 将当前进程内 LLM 缓存写入 `/user-data/llm-overrides`。
pub fn flush_llm_overrides_to_server(loc: Locale) {
    sync_llm_to_server_async(loc);
}

fn sync_llm_to_server_async(loc: Locale) {
    leptos::task::spawn_local(async move {
        let snap = with_mem(|m| {
            (
                m.api_base.clone(),
                m.model.clone(),
                m.temperature.clone(),
                m.llm_context_tokens.clone(),
                m.llm_thinking_mode.clone(),
                m.executor_api_base.clone(),
                m.executor_model.clone(),
                m.execution_mode.clone(),
                m.saved_models.clone(),
            )
        });
        let mut file = LlmOverridesDto::default();
        file.client_llm.api_base = opt_trim(&snap.0);
        file.client_llm.model = opt_trim(&snap.1);
        file.client_llm.temperature = opt_trim(&snap.2);
        file.client_llm.llm_context_tokens = opt_trim(&snap.3);
        file.client_llm.llm_thinking_mode = opt_trim(&snap.4);
        file.executor_llm.api_base = opt_trim(&snap.5);
        file.executor_llm.model = opt_trim(&snap.6);
        file.execution_mode = opt_trim(&snap.7);
        file.saved_models = snap.8;
        let _ = put_llm_overrides(&file, loc).await;
    });
}

/// 是否已配置主模型 API Key（磁盘或本进程内存）。
pub fn client_llm_storage_has_api_key() -> bool {
    with_mem(|m| m.client_key_on_server || !m.api_key.trim().is_empty())
}

pub fn load_client_llm_text_fields_from_storage() -> (String, String, String, String, String) {
    with_mem(|m| {
        (
            m.api_base.clone(),
            m.model.clone(),
            m.temperature.clone(),
            m.llm_context_tokens.clone(),
            m.llm_thinking_mode.clone(),
        )
    })
}

pub fn load_execution_mode_from_storage() -> String {
    with_mem(|m| m.execution_mode.clone())
}

pub fn persist_client_llm_to_storage(
    api_base: &str,
    model: &str,
    temperature: &str,
    llm_context_tokens: &str,
    llm_thinking_mode: &str,
    api_key_update: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    with_mem_mut(|m| {
        m.api_base = api_base.trim().to_string();
        m.model = model.trim().to_string();
        m.temperature = temperature.trim().to_string();
        m.llm_context_tokens = llm_context_tokens.trim().to_string();
        m.llm_thinking_mode = llm_thinking_mode.trim().to_string();
        if let Some(k) = api_key_update {
            m.api_key = k.trim().to_string();
            m.client_key_on_server = !m.api_key.is_empty();
        }
    });
    sync_llm_to_server_async(loc);
    if let Some(k) = api_key_update {
        let key = k.trim().to_string();
        leptos::task::spawn_local(async move {
            let _ = put_secret_client_llm(&key, loc).await;
        });
    }
    Ok(())
}

pub fn clear_client_llm_api_key_storage(loc: Locale) -> Result<(), String> {
    with_mem_mut(|m| {
        m.api_key.clear();
        m.client_key_on_server = false;
    });
    leptos::task::spawn_local(async move {
        let _ = put_secret_client_llm("", loc).await;
    });
    Ok(())
}

pub fn client_llm_json_for_chat_body() -> Option<Value> {
    with_mem(|m| {
        let mut map = serde_json::Map::new();
        if !m.api_base.trim().is_empty() {
            map.insert("api_base".into(), Value::String(m.api_base.clone()));
        }
        if !m.model.trim().is_empty() {
            map.insert("model".into(), Value::String(m.model.clone()));
        }
        if let Ok(n) = m.llm_context_tokens.trim().parse::<u64>() {
            if n > 0 {
                map.insert("llm_context_tokens".into(), Value::Number(n.into()));
            }
        }
        let tm = m.llm_thinking_mode.trim();
        if tm == "on" || tm == "off" {
            map.insert("llm_thinking_mode".into(), Value::String(tm.to_string()));
        }
        if !m.api_key.trim().is_empty() {
            map.insert("api_key".into(), Value::String(m.api_key.clone()));
        }
        if map.is_empty() {
            None
        } else {
            Some(Value::Object(map))
        }
    })
}

pub fn chat_temperature_override_from_storage() -> Option<f64> {
    let raw = with_mem(|m| m.temperature.clone());
    let parsed = raw.trim().parse::<f64>().ok()?;
    if !parsed.is_finite() || !(0.0..=2.0).contains(&parsed) {
        return None;
    }
    Some(parsed)
}

pub fn executor_llm_json_for_chat_body() -> Option<Value> {
    with_mem(|m| {
        let mut map = serde_json::Map::new();
        if !m.executor_api_base.trim().is_empty() {
            map.insert(
                "api_base".into(),
                Value::String(m.executor_api_base.clone()),
            );
        }
        if !m.executor_model.trim().is_empty() {
            map.insert("model".into(), Value::String(m.executor_model.clone()));
        }
        if !m.executor_api_key.trim().is_empty() {
            map.insert("api_key".into(), Value::String(m.executor_api_key.clone()));
        }
        if map.is_empty() {
            None
        } else {
            Some(Value::Object(map))
        }
    })
}

pub fn executor_llm_storage_has_api_key() -> bool {
    with_mem(|m| m.executor_key_on_server || !m.executor_api_key.trim().is_empty())
}

pub fn load_executor_llm_text_fields_from_storage() -> (String, String) {
    with_mem(|m| (m.executor_api_base.clone(), m.executor_model.clone()))
}

pub fn persist_executor_llm_to_storage(
    api_base: &str,
    model: &str,
    api_key_update: Option<&str>,
    loc: Locale,
) -> Result<(), String> {
    with_mem_mut(|m| {
        m.executor_api_base = api_base.trim().to_string();
        m.executor_model = model.trim().to_string();
        if let Some(k) = api_key_update {
            m.executor_api_key = k.trim().to_string();
            m.executor_key_on_server = !m.executor_api_key.is_empty();
        }
    });
    sync_llm_to_server_async(loc);
    if let Some(k) = api_key_update {
        let key = k.trim().to_string();
        leptos::task::spawn_local(async move {
            let _ = put_secret_executor_llm(&key, loc).await;
        });
    }
    Ok(())
}

pub fn clear_executor_llm_api_key_storage(loc: Locale) -> Result<(), String> {
    with_mem_mut(|m| {
        m.executor_api_key.clear();
        m.executor_key_on_server = false;
    });
    leptos::task::spawn_local(async move {
        let _ = put_secret_executor_llm("", loc).await;
    });
    Ok(())
}

pub fn persist_execution_mode_to_storage(mode: &str, loc: Locale) -> Result<(), String> {
    with_mem_mut(|m| m.execution_mode = mode.trim().to_string());
    sync_llm_to_server_async(loc);
    Ok(())
}

pub fn execution_mode_for_chat_body() -> Option<String> {
    let mode = with_mem(|m| m.execution_mode.clone());
    match mode.trim() {
        "rolling_planning" => Some("rolling_planning".to_string()),
        "hierarchical" => Some("hierarchical".to_string()),
        _ => None,
    }
}

pub fn load_readonly_tool_ttl_cache_follow_server_from_memory() -> bool {
    with_mem(|m| m.readonly_ttl_follow_server)
}

pub fn set_readonly_tool_ttl_cache_follow_server_in_memory(follow: bool) {
    with_mem_mut(|m| m.readonly_ttl_follow_server = follow);
}

pub fn persist_readonly_tool_ttl_cache_follow_server(follow: bool, loc: Locale) {
    set_readonly_tool_ttl_cache_follow_server_in_memory(follow);
    leptos::task::spawn_local(async move {
        let mut prefs = super::user_data::fetch_user_data_prefs(loc)
            .await
            .unwrap_or_default();
        prefs.disable_readonly_tool_ttl_cache = Some(!follow);
        let _ = super::user_data::put_user_data_prefs(&prefs, loc).await;
    });
}

/// 兼容旧名。
pub fn load_readonly_tool_ttl_cache_follow_server_from_storage() -> bool {
    load_readonly_tool_ttl_cache_follow_server_from_memory()
}

/// 合并进 `/chat/stream` 的 `readonly_tool_ttl_cache_secs`（关闭时返回 `Some(0)`）。
pub fn readonly_tool_ttl_cache_secs_for_chat_body() -> Option<u64> {
    if load_readonly_tool_ttl_cache_follow_server_from_memory() {
        None
    } else {
        Some(0)
    }
}
