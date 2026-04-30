//! P 步：向模型要本轮输出（含重试）。

use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc;

use crate::config::AgentConfig;
use crate::llm::{
    CompleteChatRetryingParams, LlmCompleteError, LlmRetryingTransportOpts, complete_chat_retrying,
    tool_chat_request,
};
use crate::types::{LlmSeedOverride, Message};

/// P：构造请求并调用模型（`no_stream` 为 true 时走 `stream: false`），**不**修改 `messages`。
pub(crate) struct PerPlanCallModelParams<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    /// 当前 api_key（executor 阶段可能被覆盖）
    pub api_key: &'a str,
    pub cfg: &'a AgentConfig,
    /// 默认全量工具；分阶段步级子代理时传入收窄后的切片。
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a [Message],
    pub out: Option<&'a mpsc::Sender<String>>,
    pub render_to_terminal: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub temperature_override: Option<f32>,
    pub model_override: Option<&'a str>,
    pub seed_override: LlmSeedOverride,
    pub request_chrome_trace: Option<std::sync::Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_base。
    pub executor_api_base: Option<&'a str>,
    /// 当 use_executor_model 为 true 时，优先使用此 api_key。
    pub executor_api_key: Option<&'a str>,
}

pub(crate) async fn per_plan_call_model_retrying(
    p: PerPlanCallModelParams<'_>,
) -> Result<(Message, String), LlmCompleteError> {
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
        model_override,
        seed_override,
        request_chrome_trace,
        executor_api_base,
        executor_api_key,
    } = p;

    // 确定 effective api_base 和 api_key
    let (effective_cfg, effective_api_key) =
        if executor_api_base.is_some() || executor_api_key.is_some() {
            let mut c = (*cfg).clone();
            let mut key = api_key.to_string();
            if let Some(base) = executor_api_base {
                c.api_base = base.to_string();
            }
            if let Some(key_override) = executor_api_key {
                key = key_override.to_string();
            }
            (std::sync::Arc::new(c), key)
        } else {
            (std::sync::Arc::new(cfg.clone()), api_key.to_string())
        };

    let req = tool_chat_request(
        &effective_cfg,
        messages,
        tools_defs,
        temperature_override,
        model_override,
        seed_override,
    );
    let cc = CompleteChatRetryingParams::new(
        llm_backend,
        client,
        &effective_api_key,
        &effective_cfg,
        LlmRetryingTransportOpts {
            out,
            render_to_terminal,
            no_stream,
            cancel,
            plain_terminal_stream,
        },
        request_chrome_trace,
        model_override,
    );
    let (msg, finish_reason) = complete_chat_retrying(&cc, &req).await?;
    Ok((msg, finish_reason))
}
