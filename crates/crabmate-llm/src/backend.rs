//! 可插拔的 **chat/completions** 调用后端 trait。
//!
//! 默认 OpenAI 兼容 HTTP 实现由根包 **`crabmate::llm`** 提供（[`OpenAiCompatBackend`]）。

use async_trait::async_trait;

use crabmate_types::{ChatRequest, Message};

use super::chat_params::StreamChatParams;

/// 单次 chat/completions 调用（流式 SSE 或 `stream: false` JSON），返回 assistant [`Message`] 与 `finish_reason` 字符串。
#[async_trait]
pub trait ChatCompletionsBackend: Send + Sync {
    async fn stream_chat(
        &self,
        params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>>;
}
