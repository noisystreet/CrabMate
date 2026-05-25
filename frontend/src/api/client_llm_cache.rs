//! 进程内 LLM 覆盖缓存（持久化在 **`/user-data/llm-overrides`** 与 **`secrets/*`**）。

use std::cell::RefCell;

use serde_json::Value;

thread_local! {
    static LLM_MEM: RefCell<LlmMem> = RefCell::new(LlmMem::default());
}

#[derive(Clone, Default)]
pub(crate) struct LlmMem {
    pub(crate) api_base: String,
    pub(crate) model: String,
    pub(crate) temperature: String,
    pub(crate) llm_context_tokens: String,
    pub(crate) llm_thinking_mode: String,
    pub(crate) api_key: String,
    pub(crate) executor_api_base: String,
    pub(crate) executor_model: String,
    pub(crate) executor_api_key: String,
    pub(crate) execution_mode: String,
    pub(crate) readonly_ttl_follow_server: bool,
    pub(crate) client_key_on_server: bool,
    pub(crate) executor_key_on_server: bool,
    pub(crate) saved_models: Vec<Value>,
}

pub fn with_mem<R>(f: impl FnOnce(&LlmMem) -> R) -> R {
    LLM_MEM.with(|c| f(&c.borrow()))
}

pub fn with_mem_mut<R>(f: impl FnOnce(&mut LlmMem) -> R) -> R {
    LLM_MEM.with(|c| f(&mut c.borrow_mut()))
}

pub async fn hydrate_from_server(loc: crate::i18n::Locale) {
    use super::user_data::{fetch_llm_overrides, fetch_user_data_prefs};
    let file = fetch_llm_overrides(loc).await.unwrap_or_default();
    let _prefs = fetch_user_data_prefs(loc).await;
    with_mem_mut(|m| {
        m.api_base = file.client_llm.api_base.unwrap_or_default();
        m.model = file.client_llm.model.unwrap_or_default();
        m.temperature = file.client_llm.temperature.unwrap_or_default();
        m.llm_context_tokens = file.client_llm.llm_context_tokens.unwrap_or_default();
        m.llm_thinking_mode = file.client_llm.llm_thinking_mode.unwrap_or_default();
        m.executor_api_base = file.executor_llm.api_base.unwrap_or_default();
        m.executor_model = file.executor_llm.model.unwrap_or_default();
        m.execution_mode = file.execution_mode.unwrap_or_default();
        m.saved_models = file.saved_models;
        if let Some(d) = _prefs.ok().and_then(|p| p.disable_readonly_tool_ttl_cache) {
            m.readonly_ttl_follow_server = !d;
        }
    });
    if let Ok(st) = super::user_data::fetch_secrets_status(loc).await {
        with_mem_mut(|m| {
            m.client_key_on_server = st.client_llm.set;
            m.executor_key_on_server = st.executor_llm.set;
        });
    }
}
