//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的封装层。
//!
//! - **重试**：[`complete_chat_retrying`] 实现于 **`cm_llm`**，经 [`retry_hooks::CrabmateLlmRetryHooks`] 注入 turn replay。
//! - **单次 HTTP**：[`crate::cm_llm::stream_chat`] 经 [`stream_host_impl::CrabmateStreamChatHost`] 注入 SSE 控制面。
//!
//! Agent 主循环应通过 [`complete_chat_retrying`] 发请求，避免在 `agent::agent_turn` 中散落重试与请求拼装逻辑。

mod chat_params_ext;
mod retry_hooks;
mod stream_host_impl;

pub mod backend {
    pub use crate::cm_llm::backend::ChatCompletionsBackend;
    pub use crate::cm_llm::backend_openai::{
        OPENAI_COMPAT_BACKEND, OpenAiCompatBackend, default_chat_completions_backend,
    };
    pub use crate::cm_llm::backend_shared::shared_static_chat_backend;
}

pub use backend::{
    ChatCompletionsBackend, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    default_chat_completions_backend, shared_static_chat_backend,
};
pub use chat_params_ext::CompleteChatRetryingParams;
#[allow(unused_imports)]
pub use crate::cm_llm::{
    E2eMode, FileTraceSink, LlmCallError, LlmCompleteError, LlmRetryHooks, LlmRetryingTransportOpts,
    LlmVendorAdapter, NullTraceSink, StreamChatHost, StreamChatParams, TraceEvent, TraceSink,
    TraceUsage, build_e2e_backend, chat_request_vendor_extensions_for_agent,
    conversation_messages_to_vendor_body, detect_mode_from_env, fetch_models_report,
    flatten_image_url_parts_for_config, fold_system_into_user_for_config,
    kimi_k2_5_vendor_requires_tool_call_reasoning,
    llm_vendor_adapter, llm_vendor_adapter_for_model, no_tools_chat_request,
    no_tools_chat_request_from_messages, normalize_stripped_messages_for_vendor_body, stream_chat,
    tool_chat_request, vendor, vendor_temperature_for_config,
};
#[allow(unused_imports)]
pub use stream_host_impl::{CRABMATE_STREAM_CHAT_HOST, CrabmateStreamChatHost};

use crate::cm_types::{ChatRequest, Message};

/// 调用 `chat/completions`（含指数退避重试）；Chrome trace 在根包包装层附加。
pub async fn complete_chat_retrying(
    p: &CompleteChatRetryingParams<'_>,
    request: &ChatRequest,
) -> Result<(Message, String), LlmCompleteError> {
    if let Some(budget) = p.turn_budget
        && let Err(msg) = budget.deny_llm_call_if_exhausted(&p.cfg.turn_budget)
    {
        return Err(LlmCompleteError::Other(msg.into()));
    }
    let _llm_trace = p
        .request_chrome_trace
        .as_ref()
        .map(|t| t.enter_section("llm.chat_completions"));
    let hooks = retry_hooks::CrabmateLlmRetryHooks;
    let llm_cfg = crate::cm_types::llm_config::LlmConfig {
        llm: p.cfg.llm.clone(),
        sampling: p.cfg.llm_sampling.clone(),
        vendor_flags: p.cfg.llm_vendor_flags.clone(),
        http_retry: p.cfg.llm_http_retry.clone(),
    };
    let core = crate::cm_llm::CompleteChatRetryingParams {
        llm_backend: p.llm_backend,
        stream: p.stream_params(),
        cfg: &llm_cfg,
    };
    let result = crate::cm_llm::complete_chat_retrying(&core, &hooks, request).await;
    if result.is_ok()
        && let Some(budget) = p.turn_budget
    {
        budget.record_llm_call();
        if let Ok((ref msg, _)) = result
            && let Some(tokens) =
                crate::agent::tiktoken_prompt_tokens::estimate_chat_exchange_tokens(
                    p.cfg,
                    &request.messages,
                    msg,
                )
        {
            budget.record_estimated_tokens(tokens);
        }
        budget.maybe_activate_degradation(&p.cfg.turn_budget);
    }
    result
}
