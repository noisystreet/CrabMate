//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的封装层。
//!
//! - **`api`**：单次 HTTP + SSE/JSON 解析 + 可选终端 Markdown 渲染（传输与协议细节）。
//! - **`backend`**：[`ChatCompletionsBackend`] 可插拔抽象，默认 [`OpenAiCompatBackend`]（即 `api::stream_chat`）。
//! - **重试**：[`complete_chat_retrying`] 实现于 **`crabmate-llm`**，经 [`retry_hooks::CrabmateLlmRetryHooks`] 注入 turn replay / DSML。
//!
//! Agent 主循环应通过 [`complete_chat_retrying`] 发请求，避免在 `agent::agent_turn` 中散落重试与请求拼装逻辑。

mod api;
mod backend_openai;
mod chat_params_ext;
mod retry_hooks;

pub mod backend {
    pub use super::backend_openai::{
        OPENAI_COMPAT_BACKEND, OpenAiCompatBackend, default_chat_completions_backend,
    };
    pub use crabmate_llm::backend::ChatCompletionsBackend;
}

pub use backend::{
    ChatCompletionsBackend, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    default_chat_completions_backend,
};
pub use chat_params_ext::CompleteChatRetryingParams;
#[allow(unused_imports)]
pub use crabmate_llm::{
    LlmCallError, LlmCompleteError, LlmRetryHooks, LlmRetryingTransportOpts, LlmVendorAdapter,
    StreamChatParams, TuiLlmStreamScratchArc, chat_request_vendor_extensions_for_agent,
    conversation_messages_to_vendor_body, fetch_models_report, fold_system_into_user_for_config,
    kimi_k2_5_vendor_requires_tool_call_reasoning, llm_vendor_adapter,
    llm_vendor_adapter_for_model, no_tools_chat_request,
    no_tools_chat_request_for_hierarchical_manager, no_tools_chat_request_from_messages,
    normalize_stripped_messages_for_vendor_body, tool_chat_request, vendor,
    vendor_temperature_for_config, vendor_temperature_for_model,
};

pub(crate) use crabmate_llm::STAGED_PLANNER_MIN_COMPLETION_TOKENS;

use crabmate_types::{ChatRequest, Message};

/// 调用 `chat/completions`（含指数退避重试）；Chrome trace 在根包包装层附加。
pub async fn complete_chat_retrying(
    p: &CompleteChatRetryingParams<'_>,
    request: &ChatRequest,
) -> Result<(Message, String), LlmCompleteError> {
    let _llm_trace = p
        .request_chrome_trace
        .as_ref()
        .map(|t| t.enter_section("llm.chat_completions"));
    let hooks = retry_hooks::CrabmateLlmRetryHooks { cfg: p.cfg };
    let core = crabmate_llm::CompleteChatRetryingParams {
        llm_backend: p.llm_backend,
        stream: p.stream_params(),
        cfg: p.cfg,
    };
    crabmate_llm::complete_chat_retrying(&core, &hooks, request).await
}
