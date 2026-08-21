//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的核心封装（厂商适配、HTTP 客户端、错误类型、可插拔后端 trait）。
//!
//! 带重试的 [`complete_chat_retrying`] 经 [`LlmRetryHooks`] 注入 turn replay 等宿主侧效应；
//! 单次 HTTP [`stream_chat`] 经 [`StreamChatHost`] 注入 SSE 控制面。

pub mod api;
pub mod backend;
pub mod backend_openai;
pub mod backend_shared;
pub mod cache_stats;
pub mod call_error;
pub mod chat_params;
mod complete_error;
pub mod fingerprint;
pub mod http_client;
mod openai_models;
pub mod outbound_images;
pub mod recording;
pub mod requests;
mod retry;
pub mod retry_hooks;
pub mod stream_host;
pub mod trace_sink;
pub mod vendor;
pub mod vendor_catalog;
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
pub use fingerprint::RequestFingerprint;
pub use http_client::{
    build_shared_api_client, format_reqwest_transport_err, map_reqwest_transport_err,
};
pub use openai_models::fetch_models_report;
pub use recording::{
    E2eMode, RecordingBackend, RecordingManifest, ReplayBackend, build_e2e_backend,
    detect_mode_from_env,
};
pub use requests::{
    chat_request_vendor_extensions_for_agent, kimi_k2_5_vendor_requires_tool_call_reasoning,
    no_tools_chat_request, no_tools_chat_request_from_messages, tool_chat_request,
    vendor_temperature_for_config,
};
pub use retry::{CompleteChatRetryingParams, complete_chat_retrying};
pub use retry_hooks::{LlmRetryDecisionPoint, LlmRetryHooks};
pub use stream_host::StreamChatHost;
pub use trace_sink::{FileTraceSink, NullTraceSink, TraceEvent, TraceSink, TraceUsage};
pub use vendor::{
    LlmVendorAdapter, fold_system_into_user_for_config, llm_vendor_adapter,
    llm_vendor_adapter_for_model,
};
pub use vendor_catalog::{
    ResolvedVendorCaps, VendorAdapterId, matched_vendor_default_model, matched_vendor_id,
    matched_vendor_models, resolved_vendor_caps,
};
pub use vendor_messages::{
    conversation_messages_to_vendor_body, normalize_stripped_messages_for_vendor_body,
};
