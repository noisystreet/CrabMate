//! 可插拔的 **chat/completions** 调用后端：默认实现为 OpenAI 兼容 HTTP（`api::stream_chat`）。
//!
//! 集成方可实现 [`ChatCompletionsBackend`] 并传入 [`crate::RunAgentTurnParams::llm_backend`]，在不 fork Agent 主循环的前提下接入自建网关或其它传输层；须自行保证与现有 `Message` / `tool_calls` / SSE 消费语义一致。

use std::sync::atomic::AtomicBool;

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crate::types::{ChatRequest, Message};

use super::api;

/// 单次 chat/completions 调用（流式 SSE 或 `stream: false` JSON），返回 assistant [`Message`] 与 `finish_reason` 字符串。
///
/// 契约与 [`super::api::stream_chat`] 一致：须填充 `content` / `reasoning_content` / `tool_calls`，并在提供 `out` 时下发与现有 Web SSE 兼容的纯文本增量与控制帧（由 `api` 层编码）。
#[async_trait]
pub trait ChatCompletionsBackend: Send + Sync {
    #[allow(clippy::too_many_arguments)] // 与 `api::stream_chat` 参数表一致，聚合为结构体对外部实现者负担更大
    async fn stream_chat(
        &self,
        http: &Client,
        api_key: &str,
        api_base: &str,
        req: &mut ChatRequest,
        out: Option<&Sender<String>>,
        render_to_terminal: bool,
        no_stream: bool,
        cancel: Option<&AtomicBool>,
        plain_terminal_stream: bool,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>>;
}

/// 默认后端：`POST {api_base}/chat/completions`，Bearer 鉴权，行为见 [`api::stream_chat`]。
#[derive(Debug, Copy, Clone, Default)]
pub struct OpenAiCompatBackend;

#[async_trait]
impl ChatCompletionsBackend for OpenAiCompatBackend {
    async fn stream_chat(
        &self,
        http: &Client,
        api_key: &str,
        api_base: &str,
        req: &mut ChatRequest,
        out: Option<&Sender<String>>,
        render_to_terminal: bool,
        no_stream: bool,
        cancel: Option<&AtomicBool>,
        plain_terminal_stream: bool,
    ) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
        api::stream_chat(
            http,
            api_key,
            api_base,
            req,
            out,
            render_to_terminal,
            no_stream,
            cancel,
            plain_terminal_stream,
        )
        .await
    }
}

/// 进程内默认后端实例（OpenAI 兼容 HTTP）；可与 [`default_chat_completions_backend`] 或 `&OPENAI_COMPAT_BACKEND` 互换使用。
pub static OPENAI_COMPAT_BACKEND: OpenAiCompatBackend = OpenAiCompatBackend;

/// 进程内默认后端（OpenAI 兼容 HTTP）；与未设置 `RunAgentTurnParams::llm_backend` 时行为一致。
pub fn default_chat_completions_backend() -> &'static (dyn ChatCompletionsBackend + 'static) {
    &OPENAI_COMPAT_BACKEND
}
