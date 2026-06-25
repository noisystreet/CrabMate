//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的核心封装（厂商适配、HTTP 客户端、错误类型、可插拔后端 trait）。
//!
//! 带重试的 [`complete_chat_retrying`]、单次 HTTP [`stream_chat`] 与 `ChatRequest` 惯用构造仍由根包 **`crabmate::llm`** 再导出（依赖 Agent 消息管道与 SSE 控制面）。

pub mod backend;
pub mod call_error;
pub mod chat_params;
mod complete_error;
pub mod http_client;
mod openai_models;
pub mod stream_scratch;
pub mod vendor;

pub use backend::ChatCompletionsBackend;
pub use call_error::LlmCallError;
pub use chat_params::{LlmRetryingTransportOpts, StreamChatParams};
pub use complete_error::LlmCompleteError;
pub use http_client::{
    build_shared_api_client, format_reqwest_transport_err, map_reqwest_transport_err,
};
pub use openai_models::fetch_models_report;
pub use stream_scratch::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
pub use vendor::{
    LlmVendorAdapter, fold_system_into_user_for_config, llm_vendor_adapter,
    llm_vendor_adapter_for_model,
};

/// 分阶段规划轮（无工具 JSON）的 `max_tokens` 下限；推理字段易占满较小完成额度。
pub const STAGED_PLANNER_MIN_COMPLETION_TOKENS: u32 = 3072;
