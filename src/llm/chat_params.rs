//! 单次 `chat/completions` 与带重试封装的入参聚合（控制长参数列表）。

use std::sync::atomic::AtomicBool;

use reqwest::Client;
use tokio::sync::mpsc::Sender;

use crate::config::{AgentConfig, LlmHttpAuthMode};

use super::backend::ChatCompletionsBackend;

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
}

impl CompleteChatRetryingParams<'_> {
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
            fold_system_into_user: self.cfg.llm_fold_system_into_user,
        }
    }
}
