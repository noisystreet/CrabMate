//! 将磁盘 `llm_overrides` 合并进 Web `client_llm` / `executor_llm` 请求体。
//!
//! **模型 `api_key`**：仅保留请求体（官方 Client 本机密钥 → `client_llm.api_key`）。
//! **不再**从服务端系统钥匙串 / `saved_model_*` / `client_llm` 槽回填（无桌面钥匙串的
//! `serve` 环境不应再探测）。进程环境 **`API_KEY`** 仍由回合编排侧作可选回退，不经本合并写入 body。

use crate::web::http_types::chat::ClientLlmBody;

use crate::user_data::{LlmEndpointOverride, LlmOverridesFile, load_llm_overrides};

fn fill_optional(dst: &mut Option<String>, src: Option<&String>) {
    if dst.as_ref().is_some_and(|s| !s.trim().is_empty()) {
        return;
    }
    if let Some(s) = src.filter(|x| !x.trim().is_empty()) {
        *dst = Some(s.clone());
    }
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

/// 请求体字段优先；磁盘仅填补空缺的 `api_base` / `model` / 采样相关项（**不含** `api_key`）。
#[must_use]
pub fn merge_client_llm_body(raw: Option<ClientLlmBody>) -> Option<ClientLlmBody> {
    let disk = load_llm_overrides();
    let mut body = raw.unwrap_or_default();
    merge_endpoint(&mut body, &disk.client_llm);
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

/// Executor LLM：磁盘 `llm_overrides.executor_llm` 填补端点；**不含**钥匙串 `api_key` 回填。
#[must_use]
pub fn merge_executor_llm_body(
    raw: Option<crate::web::http_types::chat::ExecutorLlmBody>,
) -> Option<crate::web::http_types::chat::ExecutorLlmBody> {
    let disk: LlmOverridesFile = load_llm_overrides();
    let mut body = raw.unwrap_or_default();
    fill_optional(&mut body.api_base, disk.executor_llm.api_base.as_ref());
    fill_optional(&mut body.model, disk.executor_llm.model.as_ref());
    if body.api_base.is_none() && body.model.is_none() && body.api_key.is_none() {
        return None;
    }
    Some(body)
}
