//! 将磁盘 `llm_overrides` / `secrets` 合并进 Web `client_llm` 请求体（§8 优先级 3–4，请求体 5 最高）。

use crate::web::http_types::chat::ClientLlmBody;

use super::store::{load_llm_overrides, read_secret_client_llm, read_secret_executor_llm};
use super::types::LlmOverridesFile;

fn fill_optional(dst: &mut Option<String>, src: Option<&String>) {
    if dst.as_ref().is_some_and(|s| !s.trim().is_empty()) {
        return;
    }
    if let Some(s) = src.filter(|x| !x.trim().is_empty()) {
        *dst = Some(s.clone());
    }
}

fn merge_endpoint(
    body: &mut ClientLlmBody,
    disk: &super::types::LlmEndpointOverride,
    secret_key: Option<String>,
) {
    fill_optional(&mut body.api_base, disk.api_base.as_ref());
    fill_optional(&mut body.model, disk.model.as_ref());
    if body.api_key.as_ref().is_none_or(|s| s.trim().is_empty())
        && let Some(k) = secret_key.filter(|x| !x.trim().is_empty())
    {
        body.api_key = Some(k);
    }
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

/// 请求体字段优先；磁盘仅填补空缺项。
#[must_use]
pub fn merge_client_llm_body(raw: Option<ClientLlmBody>) -> Option<ClientLlmBody> {
    let disk = load_llm_overrides();
    let secret = read_secret_client_llm();
    let mut body = raw.unwrap_or_default();
    merge_endpoint(&mut body, &disk.client_llm, secret);
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

/// Executor LLM：磁盘 `llm_overrides.executor_llm` + `secrets/executor_llm`。
#[must_use]
pub fn merge_executor_llm_body(
    raw: Option<crate::web::http_types::chat::ExecutorLlmBody>,
) -> Option<crate::web::http_types::chat::ExecutorLlmBody> {
    let disk: LlmOverridesFile = load_llm_overrides();
    let secret = read_secret_executor_llm();
    let mut body = raw.unwrap_or_default();
    fill_optional(&mut body.api_base, disk.executor_llm.api_base.as_ref());
    fill_optional(&mut body.model, disk.executor_llm.model.as_ref());
    if body.api_key.as_ref().is_none_or(|s| s.trim().is_empty())
        && let Some(k) = secret.filter(|x| !x.trim().is_empty())
    {
        body.api_key = Some(k);
    }
    if body.api_base.is_none() && body.model.is_none() && body.api_key.is_none() {
        return None;
    }
    Some(body)
}
