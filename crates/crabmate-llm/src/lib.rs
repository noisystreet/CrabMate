//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的核心封装（厂商适配、HTTP 客户端、错误类型、可插拔后端 trait）。
//!
//! 带重试的 [`complete_chat_retrying`] 经 [`LlmRetryHooks`] 注入 turn replay / DSML 等宿主侧效应；
//! 单次 HTTP [`stream_chat`] 经 [`StreamChatHost`] 注入 SSE 控制面与终端渲染。

pub mod api;
pub mod backend;
pub mod backend_openai;
pub mod backend_shared;
pub mod call_error;
pub mod chat_params;
mod complete_error;
pub mod http_client;
mod openai_models;
pub mod requests;
mod retry;
pub mod retry_hooks;
pub mod stream_host;
pub mod stream_scratch;
pub mod vendor;
pub mod vendor_messages;

pub use api::stream_chat;
pub use backend::ChatCompletionsBackend;
pub use backend_openai::{
    OPENAI_COMPAT_BACKEND, OpenAiCompatBackend, default_chat_completions_backend,
};
pub use backend_shared::{shared_chat_backend, shared_static_chat_backend};
pub use call_error::LlmCallError;
pub use chat_params::{LlmRetryingTransportOpts, StreamChatParams};
pub use complete_error::LlmCompleteError;
pub use http_client::{
    build_shared_api_client, format_reqwest_transport_err, map_reqwest_transport_err,
};
pub use openai_models::fetch_models_report;
pub use requests::{
    chat_request_vendor_extensions_for_agent, kimi_k2_5_vendor_requires_tool_call_reasoning,
    no_tools_chat_request, no_tools_chat_request_for_hierarchical_manager,
    no_tools_chat_request_from_messages, tool_chat_request, vendor_temperature_for_config,
    vendor_temperature_for_model,
};
pub use retry::{CompleteChatRetryingParams, complete_chat_retrying};
pub use retry_hooks::{LlmRetryDecisionPoint, LlmRetryHooks};
pub use stream_host::{
    CliWaitSpinnerGuardHost, DsmlStreamFilter, StreamChatHost, TerminalPlainFragmentCtx,
};
pub use stream_scratch::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
pub use vendor::{
    LlmVendorAdapter, fold_system_into_user_for_config, llm_vendor_adapter,
    llm_vendor_adapter_for_model,
};
pub use vendor_messages::{
    conversation_messages_to_vendor_body, normalize_stripped_messages_for_vendor_body,
};

/// 分阶段规划轮（无工具 JSON）的 `max_tokens` 下限；推理字段易占满较小完成额度。
pub const STAGED_PLANNER_MIN_COMPLETION_TOKENS: u32 = 3072;
