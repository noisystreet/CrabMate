//! 默认 OpenAI 兼容 HTTP 后端（根包 `api::stream_chat` 实现）。

use async_trait::async_trait;

use crabmate_llm::backend::ChatCompletionsBackend;
use crabmate_llm::chat_params::StreamChatParams;
use crabmate_types::{ChatRequest, Message};

use super::api;

/// 默认后端：`POST {api_base}/chat/completions`，Bearer 鉴权，行为见 [`api::stream_chat`]。
#[derive(Debug, Copy, Clone, Default)]
pub struct OpenAiCompatBackend;

#[async_trait]
impl ChatCompletionsBackend for OpenAiCompatBackend {
    async fn stream_chat(
        &self,
        params: &StreamChatParams<'_>,
        req: &mut ChatRequest,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        api::stream_chat(params, req).await
    }
}

/// 进程内默认后端实例（OpenAI 兼容 HTTP）。
pub static OPENAI_COMPAT_BACKEND: OpenAiCompatBackend = OpenAiCompatBackend;

/// 进程内默认后端；与未设置自定义 `llm_backend` 时行为一致。
pub fn default_chat_completions_backend() -> &'static (dyn ChatCompletionsBackend + 'static) {
    &OPENAI_COMPAT_BACKEND
}
