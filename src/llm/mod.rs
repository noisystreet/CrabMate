//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的封装层。
//!
//! - **`api`**：单次 HTTP + SSE/JSON 解析 + 可选终端 Markdown 渲染（传输与协议细节）。
//! - **`backend`**：[`ChatCompletionsBackend`] 可插拔抽象，默认 [`OpenAiCompatBackend`]（即 `api::stream_chat`）。
//! - **`vendor`**：按网关族调整出站 JSON（温度、`thinking`、是否保留 tool 轮 `reasoning_content`）；新增厂商见 [`vendor::LlmVendorAdapter`]。
//! - **本模块**：`ChatRequest` 的惯用构造、带指数退避的**重试策略**（仅对 [`call_error::LlmCallError`] 标记为 `retryable` 的失败：如 **408/429/5xx** 与部分传输错误；**401/400** 等客户端错误不重试）、以及后续可扩展的调用入口（例如统一超时、观测字段）。
//!
//! Agent 主循环应通过 [`complete_chat_retrying`] 发请求，避免在 `agent::agent_turn` 中散落重试与请求拼装逻辑。

mod api;
pub mod backend;
mod call_error;
mod chat_params;
mod openai_models;
pub mod vendor;

pub use chat_params::{CompleteChatRetryingParams, StreamChatParams};
pub use openai_models::fetch_models_report;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use log::{debug, error, info};

use crate::config::AgentConfig;
use crate::types::{
    ChatRequest, LlmSeedOverride, Message, Tool, is_long_term_memory_injection, resolved_llm_seed,
};

pub use backend::{
    ChatCompletionsBackend, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    default_chat_completions_backend,
};
pub use vendor::{LlmVendorAdapter, llm_vendor_adapter, llm_vendor_adapter_for_model};

/// **kimi-k2.5** 在**未**显式关闭思考时，服务端 **`thinking` 默认启用**；此时含 **`tool_calls`** 的 assistant 历史消息必须带 **`reasoning_content`**，否则返回 `invalid_request_error`（见 Moonshot [Chat API](https://platform.moonshot.cn/docs/api/chat) 与实测报错）。
#[inline]
pub(crate) fn kimi_k2_5_vendor_requires_tool_call_reasoning(cfg: &AgentConfig) -> bool {
    llm_vendor_adapter(cfg).preserve_assistant_tool_call_reasoning(cfg)
}

/// 按模型 ID 将出站 **`temperature`** 钳到当前 [`LlmVendorAdapter`] 允许值（见 [`llm_vendor_adapter_for_model`]；有完整配置时请用 [`vendor_temperature_for_config`] / [`llm_vendor_adapter`]）。
#[inline]
#[allow(dead_code)] // 嵌入方与单测使用；默认 `cargo build --lib` 无库内调用
pub(crate) fn vendor_temperature_for_model(model: &str, temperature: f32) -> f32 {
    llm_vendor_adapter_for_model(model).coerce_temperature(model, temperature)
}

/// 按 **`AgentConfig`**（**`model` + `api_base`**）钳制温度（摘要等路径与 [`llm_vendor_adapter`] 一致）。
#[inline]
pub(crate) fn vendor_temperature_for_config(cfg: &AgentConfig, temperature: f32) -> f32 {
    llm_vendor_adapter(cfg).coerce_temperature(&cfg.model, temperature)
}

/// 按配置生成请求体可选字段 **`thinking`**（由各厂商 [`LlmVendorAdapter::thinking_field`] 决定）。
#[inline]
pub(crate) fn chat_request_thinking_from_cfg(cfg: &AgentConfig) -> Option<serde_json::Value> {
    llm_vendor_adapter(cfg).thinking_field(cfg)
}

/// 构造带 tools、**`tool_choice: auto`** 及采样参数的请求体（`stream` 由 [`api::stream_chat`] 按 `no_stream` 覆盖）。
pub fn tool_chat_request(
    cfg: &AgentConfig,
    messages: &[Message],
    tools: &[Tool],
    temperature_override: Option<f32>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    let v = llm_vendor_adapter(cfg);
    ChatRequest {
        model: cfg.model.clone(),
        messages: crate::agent::message_pipeline::conversation_messages_to_vendor_body(
            messages,
            cfg.llm_fold_system_into_user,
            v.preserve_assistant_tool_call_reasoning(cfg),
        ),
        tools: Some(tools.to_vec()),
        tool_choice: Some("auto".to_string()),
        max_tokens: cfg.max_tokens,
        temperature: v
            .coerce_temperature(&cfg.model, temperature_override.unwrap_or(cfg.temperature)),
        seed: resolved_llm_seed(cfg.llm_seed, seed_override),
        stream: None,
        reasoning_split: cfg.llm_reasoning_split.then_some(true),
        thinking: v.thinking_field(cfg),
    }
}

/// 构造**显式禁止工具调用**的请求（`tools: []` + `tool_choice: "none"`），用于分阶段规划轮等。
/// 按 OpenAI API 语义硬性禁止模型返回 `tool_calls`，比省略 `tools` 字段（`None`）更可靠。
/// 对 `messages` 先做 [`crate::types::messages_for_api_stripping_reasoning_skip_ui_separators`] 再 normalize；进程内分阶段路径优先 [`no_tools_chat_request_from_messages`] 以避免二次 strip。
#[allow(dead_code)] // 公共 API；单测覆盖等价性，主进程分阶段路径用 `no_tools_chat_request_from_messages`
pub fn no_tools_chat_request(
    cfg: &AgentConfig,
    messages: &[Message],
    temperature_override: Option<f32>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    no_tools_chat_request_from_messages(
        cfg,
        crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(
            messages,
            kimi_k2_5_vendor_requires_tool_call_reasoning(cfg),
        ),
        temperature_override,
        seed_override,
    )
}

/// 与 [`no_tools_chat_request`] 相同，但接受**已**按规划轮规则拼好的 `messages`（通常已不含 UI 分隔线且已剥离 `reasoning_content`），再剔除 [`crate::types::is_long_term_memory_injection`]，仅经 [`crate::types::normalize_messages_for_openai_compatible_request`]，避免对同一会话再做一轮全量 `strip`。
pub fn no_tools_chat_request_from_messages(
    cfg: &AgentConfig,
    messages: Vec<Message>,
    temperature_override: Option<f32>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    let messages: Vec<Message> = messages
        .into_iter()
        .filter(|m| !is_long_term_memory_injection(m))
        .collect();
    let v = llm_vendor_adapter(cfg);
    ChatRequest {
        model: cfg.model.clone(),
        messages: crate::agent::message_pipeline::normalize_stripped_messages_for_vendor_body(
            messages,
            cfg.llm_fold_system_into_user,
        ),
        tools: Some(vec![]),
        tool_choice: Some("none".to_string()),
        max_tokens: cfg.max_tokens,
        temperature: v
            .coerce_temperature(&cfg.model, temperature_override.unwrap_or(cfg.temperature)),
        seed: resolved_llm_seed(cfg.llm_seed, seed_override),
        stream: None,
        reasoning_split: cfg.llm_reasoning_split.then_some(true),
        thinking: v.thinking_field(cfg),
    }
}

/// 调用 `chat/completions`：失败时若错误为 **可重试**（见 [`call_error::LlmCallError`]），按 `AgentConfig::api_retry_delay_secs` 做指数退避，最多 `api_max_retries + 1` 次；**401/400** 等不可重试错误立即返回。
///
/// `llm_backend` 默认使用 [`default_chat_completions_backend`]（OpenAI 兼容 HTTP）；可换为自定义 [`ChatCompletionsBackend`]。
pub async fn complete_chat_retrying(
    p: &CompleteChatRetryingParams<'_>,
    request: &ChatRequest,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let _llm_trace = p
        .request_chrome_trace
        .as_ref()
        .map(|t| t.enter_section("llm.chat_completions"));
    let t0 = Instant::now();
    let max_attempts = p.cfg.api_max_retries + 1;
    let mut last_ok = None;
    let mut req = request.clone();
    let stream = p.stream_params();
    for attempt in 0..max_attempts {
        if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(crate::types::LLM_CANCELLED_ERROR.into());
        }
        match p.llm_backend.stream_chat(&stream, &mut req).await {
            Ok(r) => {
                let (mut msg, finish_reason) = r;
                crate::text_sanitize::materialize_deepseek_dsml_tool_calls_in_message(
                    &mut msg,
                    p.cfg.materialize_deepseek_dsml_tool_calls,
                );
                info!(
                    target: "crabmate",
                    "llm chat 完成 model={} elapsed_ms={} attempt={}",
                    request.model,
                    t0.elapsed().as_millis(),
                    attempt + 1
                );
                debug!(
                    target: "crabmate",
                    "llm chat 响应摘要（含重试后成功） finish_reason={} message_in_request={} assistant_preview={}",
                    finish_reason,
                    request.messages.len(),
                    crate::redact::assistant_message_preview_for_log(&msg)
                );
                last_ok = Some((msg, finish_reason));
                break;
            }
            Err(e) => {
                let http_status = call_error::llm_call_error_http_status(e.as_ref());
                let retryable = call_error::llm_call_error_retryable(e.as_ref());
                error!(
                    target: "crabmate",
                    "llm chat 请求失败 http_status={:?} retryable={} error={} attempt={} max_attempts={}",
                    http_status,
                    retryable,
                    e,
                    attempt + 1,
                    max_attempts
                );
                let can_backoff = attempt < max_attempts - 1 && retryable;
                if can_backoff {
                    let delay_secs = p
                        .cfg
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt));
                    info!(
                        target: "crabmate",
                        "llm 等待后重试 delay_secs={}",
                        delay_secs
                    );
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                        return Err(crate::types::LLM_CANCELLED_ERROR.into());
                    }
                } else {
                    return Err(e);
                }
            }
        }
    }
    last_ok.ok_or_else(|| std::io::Error::other("llm chat 成功分支未写入结果（逻辑错误）").into())
}

#[cfg(test)]
mod tests {
    use crate::config::load_config;
    use crate::types::{
        LlmSeedOverride, Message, OPENAI_CHAT_COMPLETIONS_REL_PATH, OPENAI_MODELS_REL_PATH,
        messages_for_api_stripping_reasoning_skip_ui_separators,
    };

    #[test]
    fn completions_path_matches_openai_compat() {
        assert_eq!(OPENAI_CHAT_COMPLETIONS_REL_PATH, "chat/completions");
    }

    #[test]
    fn models_path_matches_openai_compat() {
        assert_eq!(OPENAI_MODELS_REL_PATH, "models");
    }

    #[test]
    fn no_tools_chat_request_matches_from_messages_after_strip_skip_sep() {
        let cfg = load_config(None).expect("default embedded config");
        let sep = Message::chat_ui_separator(true);
        let assistant = Message {
            role: "assistant".to_string(),
            content: Some("c".to_string()),
            reasoning_content: Some("r".to_string()),
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let messages = vec![Message::user_only("u"), sep, assistant];
        let a = super::no_tools_chat_request(&cfg, &messages, None, LlmSeedOverride::FromConfig);
        let stripped = messages_for_api_stripping_reasoning_skip_ui_separators(&messages, false);
        let b = super::no_tools_chat_request_from_messages(
            &cfg,
            stripped,
            None,
            LlmSeedOverride::FromConfig,
        );
        assert_eq!(a.messages, b.messages);
        assert_eq!(a.tool_choice, b.tool_choice);
        assert_eq!(a.tools.as_ref().map(|t| t.len()), Some(0));
    }

    #[test]
    fn tool_chat_request_coerces_temperature_for_kimi_k2_5_model() {
        let mut cfg = load_config(None).expect("default embedded config");
        cfg.model = "kimi-k2.5".to_string();
        cfg.temperature = 0.3;
        let req = super::tool_chat_request(
            &cfg,
            &[Message::user_only("hi")],
            &[],
            None,
            LlmSeedOverride::FromConfig,
        );
        assert_eq!(req.temperature, 1.0);
        let req = super::tool_chat_request(
            &cfg,
            &[Message::user_only("hi")],
            &[],
            Some(0.7),
            LlmSeedOverride::FromConfig,
        );
        assert_eq!(req.temperature, 1.0);
    }
}
