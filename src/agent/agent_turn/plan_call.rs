//! P 步：向模型要本轮输出（含重试）。

use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc;

use crate::config::AgentConfig;
use crate::llm::{complete_chat_retrying, tool_chat_request};
use crate::types::{LlmSeedOverride, Message, is_chat_ui_separator};

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
    } = p;
    let filtered: Vec<Message> = messages
        .iter()
        .filter(|m| !is_chat_ui_separator(m))
        .cloned()
        .collect();
    let req = tool_chat_request(
        cfg,
        &filtered,
        tools_defs,
        temperature_override,
        seed_override,
    );
    let (mut msg, finish_reason) = complete_chat_retrying(
        llm_backend,
        client,
        api_key,
        cfg,
        &req,
        out,
        render_to_terminal,
        no_stream,
        cancel,
        plain_terminal_stream,
    )
    .await?;
    crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(&mut msg);
    Ok((msg, finish_reason))
}
