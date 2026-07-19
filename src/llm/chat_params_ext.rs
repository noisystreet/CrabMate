//! 带重试封装的入参（含根包观测与 Chrome trace 钩子）。

use std::sync::Arc;

use crabmate_config::AgentConfig;
use crabmate_llm::backend::ChatCompletionsBackend;
use crabmate_llm::chat_params::{LlmRetryingTransportOpts, StreamChatParams};
use crabmate_llm::stream_scratch::TuiLlmStreamScratchArc;
use crabmate_llm::{fold_system_into_user_for_config, llm_vendor_adapter, vendor};
use reqwest::Client;

use crate::agent::turn_budget::TurnBudgetCounter;

/// [`super::complete_chat_retrying`] 入参（不含每次克隆前的 `ChatRequest`）。
pub struct CompleteChatRetryingParams<'a> {
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub http: &'a Client,
    pub api_key: &'a str,
    pub cfg: &'a AgentConfig,
    pub out: Option<&'a tokio::sync::mpsc::Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a std::sync::atomic::AtomicBool>,
    pub plain_terminal_stream: bool,
    pub tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
    pub request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    pub model_override: Option<&'a str>,
    /// 单轮共享预算；设置时由 [`super::complete_chat_retrying`] 统一门禁与计数。
    pub turn_budget: Option<&'a Arc<TurnBudgetCounter>>,
}

impl<'a> CompleteChatRetryingParams<'a> {
    pub fn new(
        llm_backend: &'a dyn ChatCompletionsBackend,
        http: &'a Client,
        api_key: &'a str,
        cfg: &'a AgentConfig,
        transport: LlmRetryingTransportOpts<'a>,
        request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
        model_override: Option<&'a str>,
    ) -> Self {
        let LlmRetryingTransportOpts {
            out,
            render_to_terminal,
            no_stream,
            cancel,
            plain_terminal_stream,
            tui_llm_stream_scratch,
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
            tui_llm_stream_scratch,
            request_chrome_trace,
            model_override,
            turn_budget: None,
        }
    }

    /// 携带与主 Agent 回合共用的 [`TurnBudgetCounter`]。
    #[inline]
    pub fn with_turn_budget(mut self, turn_budget: Option<&'a Arc<TurnBudgetCounter>>) -> Self {
        self.turn_budget = turn_budget;
        self
    }

    pub(crate) fn stream_params(&self) -> StreamChatParams<'_> {
        // `vendor` 接口不再需要 `AgentConfig`，此处构造临时 `LlmConfig` 供 trait 方法使用。
        let llm_cfg = crabmate_types::llm_config::LlmConfig {
            llm: self.cfg.llm.clone(),
            sampling: self.cfg.llm_sampling.clone(),
            vendor_flags: self.cfg.llm_vendor_flags.clone(),
            http_retry: self.cfg.llm_http_retry.clone(),
        };
        StreamChatParams {
            host: &crate::llm::stream_host_impl::CRABMATE_STREAM_CHAT_HOST,
            client: self.http,
            api_key: self.api_key,
            api_base: &self.cfg.llm.api_base,
            auth_mode: self.cfg.llm.llm_http_auth_mode,
            out: self.out,
            render_to_terminal: self.render_to_terminal,
            no_stream: self.no_stream,
            cancel: self.cancel,
            plain_terminal_stream: self.plain_terminal_stream,
            fold_system_into_user: fold_system_into_user_for_config(
                &self.cfg.llm.model,
                &self.cfg.llm.api_base,
            ),
            preserve_reasoning_on_assistant_tool_calls: llm_vendor_adapter(
                &self.cfg.llm.model,
                &self.cfg.llm.api_base,
            )
            .preserve_assistant_tool_call_reasoning(&llm_cfg),
            preserve_deepseek_thinking_reasoning_roundtrip: vendor::deepseek_json_output_eligible(
                &self.cfg.llm.api_base,
            ),
            thinking_trace_enabled: self.cfg.agent_thinking_trace.agent_thinking_trace_enabled,
            dsml_stream_strip_enabled: self.cfg.dsml_materialize.dsml_stream_strip_enabled,
            tui_llm_stream_scratch: self.tui_llm_stream_scratch.clone(),
        }
    }
}
