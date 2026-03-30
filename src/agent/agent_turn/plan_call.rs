//! P 步：向模型要本轮输出（含重试）。

use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc;

use crate::config::AgentConfig;
use crate::llm::{CompleteChatRetryingParams, complete_chat_retrying, tool_chat_request};
use crate::types::{LlmSeedOverride, Message};

/// P：构造请求并调用模型（`no_stream` 为 true 时走 `stream: false`），**不**修改 `messages`。
pub(crate) struct PerPlanCallModelParams<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a AgentConfig,
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a [Message],
    pub out: Option<&'a mpsc::Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    pub request_chrome_trace: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
}

pub(crate) async fn per_plan_call_model_retrying(
    p: PerPlanCallModelParams<'_>,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let PerPlanCallModelParams {
        llm_backend,
        client,
        api_key,
        cfg,
        tools_defs,
        messages,
        out,
        render_to_terminal,
        no_stream,
        cancel,
        plain_terminal_stream,
        temperature_override,
        seed_override,
        request_chrome_trace,
    } = p;
    let req = tool_chat_request(
        cfg,
        messages,
        tools_defs,
        temperature_override,
        seed_override,
    );
    let cc = CompleteChatRetryingParams {
        llm_backend,
        http: client,
        api_key,
        cfg,
        out,
        render_to_terminal,
        no_stream,
        cancel,
        plain_terminal_stream,
        request_chrome_trace,
    };
    let (msg, finish_reason) = complete_chat_retrying(&cc, &req).await?;
    Ok((msg, finish_reason))
}
