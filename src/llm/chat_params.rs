//! 单次 `chat/completions` 与带重试封装的入参聚合（控制长参数列表）。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crate::config::{AgentConfig, LlmHttpAuthMode};

use super::backend::ChatCompletionsBackend;

/// 与 [`CompleteChatRetryingParams`] 配套的 **SSE / 终端 / 流式 / 取消** 开关（各调用点差异主要在此）。
#[derive(Clone, Copy)]
pub struct LlmRetryingTransportOpts<'a> {
    pub out: Option<&'a Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
}

impl<'a> LlmRetryingTransportOpts<'a> {
    /// 无 SSE、非流式、无取消（如上下文摘要等后台 `complete_chat_retrying`）。
    pub fn headless_no_stream() -> Self {
        Self {
            out: None,
            render_to_terminal: false,
            no_stream: true,
            cancel: None,
            plain_terminal_stream: false,
        }
    }
}

/// 与 [`super::api::stream_chat`] 一致的传输与展示开关（不含可变请求体）。
#[derive(Clone, Copy)]
pub struct StreamChatParams<'a> {
    pub client: &'a Client,
    pub api_key: &'a str,
    pub api_base: &'a str,
    pub auth_mode: LlmHttpAuthMode,
    pub out: Option<&'a Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub fold_system_into_user: bool,
    /// Moonshot **kimi-k2.5** + 默认 thinking：含 **`tool_calls`** 的 assistant 须保留 **`reasoning_content`**（见 [`super::vendor::LlmVendorAdapter::preserve_assistant_tool_call_reasoning`]）。
    pub preserve_reasoning_on_assistant_tool_calls: bool,
    /// 为 true 时经 SSE 下发结构化 **`thinking_trace`**（推理增量、终答阶段等），供 Web 调试台。
    pub thinking_trace_enabled: bool,
}

/// [`super::complete_chat_retrying`] 入参（不含每次克隆前的 `ChatRequest`）。
pub struct CompleteChatRetryingParams<'a> {
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub http: &'a Client,
    pub api_key: &'a str,
    pub cfg: &'a AgentConfig,
    pub out: Option<&'a Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
}

impl<'a> CompleteChatRetryingParams<'a> {
    /// 拼装 [`CompleteChatRetryingParams`]，避免各 `agent` 调用点重复写字段。
    pub fn new(
        llm_backend: &'a dyn ChatCompletionsBackend,
        http: &'a Client,
        api_key: &'a str,
        cfg: &'a AgentConfig,
        transport: LlmRetryingTransportOpts<'a>,
        request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    ) -> Self {
        let LlmRetryingTransportOpts {
            out,
            render_to_terminal,
            no_stream,
            cancel,
            plain_terminal_stream,
        } = transport;
        Self {
            llm_backend,
            http,
            api_key,
            cfg,
            out,
            render_to_terminal,
            no_stream,
            cancel,
            plain_terminal_stream,
            request_chrome_trace,
        }
    }

    pub(crate) fn stream_params(&self) -> StreamChatParams<'_> {
        StreamChatParams {
            client: self.http,
            api_key: self.api_key,
            api_base: &self.cfg.api_base,
            auth_mode: self.cfg.llm_http_auth_mode,
            out: self.out,
            render_to_terminal: self.render_to_terminal,
            no_stream: self.no_stream,
            cancel: self.cancel,
            plain_terminal_stream: self.plain_terminal_stream,
            fold_system_into_user: super::fold_system_into_user_for_config(self.cfg),
            preserve_reasoning_on_assistant_tool_calls: super::llm_vendor_adapter(self.cfg)
                .preserve_assistant_tool_call_reasoning(self.cfg),
            thinking_trace_enabled: self.cfg.agent_thinking_trace_enabled,
        }
    }
}
