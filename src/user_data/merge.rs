//! 将磁盘 `llm_overrides` / 系统钥匙串合并进 Web `client_llm` 请求体。
//!
//! **`api_key` 优先级**（高 → 低）：请求体 → 环境变量 **`API_KEY`**（与 CLI 启动一致）→
//! 已保存模型钥匙串 → `client_llm` / `executor_llm` 钥匙串。

use crate::web::http_types::chat::ClientLlmBody;

use crate::user_data::{
    LlmEndpointOverride, LlmOverridesFile, load_llm_overrides, read_saved_model_secret,
    read_secret_client_llm, read_secret_executor_llm,
};

fn fill_optional(dst: &mut Option<String>, src: Option<&String>) {
    if dst.as_ref().is_some_and(|s| !s.trim().is_empty()) {
        return;
    }
    if let Some(s) = src.filter(|x| !x.trim().is_empty()) {
        *dst = Some(s.clone());
    }
}

fn env_api_key_is_set() -> bool {
    std::env::var("API_KEY").is_ok_and(|value| !value.trim().is_empty())
}

fn merge_endpoint(body: &mut ClientLlmBody, disk: &LlmEndpointOverride) {
    fill_optional(&mut body.api_base, disk.api_base.as_ref());
    fill_optional(&mut body.model, disk.model.as_ref());
    if body.llm_context_tokens.is_none()
        && let Some(ref t) = disk.llm_context_tokens
        && let Ok(n) = t.trim().parse::<u64>()
    {
        body.llm_context_tokens = Some(n);
    }
    if body
        .llm_thinking_mode
        .as_ref()
        .is_none_or(|s| s.trim().is_empty())
    {
        fill_optional(&mut body.llm_thinking_mode, disk.llm_thinking_mode.as_ref());
    }
}

fn body_api_key_empty(api_key: &Option<String>) -> bool {
    api_key.as_ref().is_none_or(|key| key.trim().is_empty())
}

/// 请求体字段优先；磁盘 / 钥匙串仅填补空缺项（`api_key` 在环境变量已设置时不覆盖）。
#[must_use]
pub fn merge_client_llm_body(raw: Option<ClientLlmBody>) -> Option<ClientLlmBody> {
    let disk = load_llm_overrides();
    let mut body = raw.unwrap_or_default();
    merge_endpoint(&mut body, &disk.client_llm);
    // 与 CLI 一致：进程环境 `API_KEY` 优先于本机钥匙串，便于临时覆盖而不改持久密钥。
    if body_api_key_empty(&body.api_key) && !env_api_key_is_set() {
        body.api_key = read_saved_model_secret(
            &disk.saved_models,
            body.api_base.as_deref(),
            body.model.as_deref(),
        );
        if body_api_key_empty(&body.api_key) {
            body.api_key = read_secret_client_llm();
        }
    }
    if body.api_base.is_none()
        && body.model.is_none()
        && body.api_key.is_none()
        && body.llm_context_tokens.is_none()
        && body.llm_thinking_mode.is_none()
    {
        return None;
    }
    Some(body)
}

/// Executor LLM：磁盘 `llm_overrides.executor_llm` + 系统钥匙串 `executor_llm`。
#[must_use]
pub fn merge_executor_llm_body(
    raw: Option<crate::web::http_types::chat::ExecutorLlmBody>,
) -> Option<crate::web::http_types::chat::ExecutorLlmBody> {
    let disk: LlmOverridesFile = load_llm_overrides();
    let mut body = raw.unwrap_or_default();
    fill_optional(&mut body.api_base, disk.executor_llm.api_base.as_ref());
    fill_optional(&mut body.model, disk.executor_llm.model.as_ref());
    if body_api_key_empty(&body.api_key) {
        body.api_key = read_saved_model_secret(
            &disk.saved_models,
            body.api_base.as_deref(),
            body.model.as_deref(),
        );
    }
    if body_api_key_empty(&body.api_key) {
        body.api_key = read_secret_executor_llm();
    }
    if body.api_base.is_none() && body.model.is_none() && body.api_key.is_none() {
        return None;
    }
    Some(body)
}
