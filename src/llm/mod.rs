//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的封装层。
//!
//! - **`api`**：单次 HTTP + SSE/JSON 解析 + 可选终端 Markdown 渲染（传输与协议细节）。
//! - **`backend`**：[`ChatCompletionsBackend`] 可插拔抽象，默认 [`OpenAiCompatBackend`]（即 `api::stream_chat`）。
//! - **本模块**：`ChatRequest` 的惯用构造、带指数退避的**重试策略**、以及后续可扩展的调用入口（例如统一超时、观测字段）。
//!
//! Agent 主循环应通过 [`complete_chat_retrying`] 发请求，避免在 `agent::agent_turn` 中散落重试与请求拼装逻辑。

mod api;
pub mod backend;
mod openai_models;

pub use openai_models::fetch_models_report;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::{debug, error, info};
use tokio::sync::mpsc::Sender;

use crate::config::AgentConfig;
use crate::types::{
    ChatRequest, LlmSeedOverride, Message, Tool, is_long_term_memory_injection, resolved_llm_seed,
};
use reqwest::Client;

pub use backend::{
    ChatCompletionsBackend, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    default_chat_completions_backend,
};

/// 构造带 tools、**`tool_choice: auto`** 及采样参数的请求体（`stream` 由 [`api::stream_chat`] 按 `no_stream` 覆盖）。
pub fn tool_chat_request(
    cfg: &AgentConfig,
    messages: &[Message],
    tools: &[Tool],
    temperature_override: Option<f32>,
    seed_override: LlmSeedOverride,
) -> ChatRequest {
    ChatRequest {
        model: cfg.model.clone(),
        messages: crate::types::normalize_messages_for_openai_compatible_request(
            crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(messages),
        ),
        tools: Some(tools.to_vec()),
        tool_choice: Some("auto".to_string()),
        max_tokens: cfg.max_tokens,
        temperature: temperature_override.unwrap_or(cfg.temperature),
        seed: resolved_llm_seed(cfg.llm_seed, seed_override),
        stream: None,
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
        crate::types::messages_for_api_stripping_reasoning_skip_ui_separators(messages),
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
    ChatRequest {
        model: cfg.model.clone(),
        messages: crate::types::normalize_messages_for_openai_compatible_request(messages),
        tools: Some(vec![]),
        tool_choice: Some("none".to_string()),
        max_tokens: cfg.max_tokens,
        temperature: temperature_override.unwrap_or(cfg.temperature),
        seed: resolved_llm_seed(cfg.llm_seed, seed_override),
        stream: None,
    }
}

/// 调用 `chat/completions`：失败时按 `AgentConfig::api_retry_delay_secs` 做指数退避，最多 `api_max_retries + 1` 次。
///
/// `llm_backend` 默认使用 [`default_chat_completions_backend`]（OpenAI 兼容 HTTP）；可换为自定义 [`ChatCompletionsBackend`]。
#[allow(clippy::too_many_arguments)]
pub async fn complete_chat_retrying(
    llm_backend: &dyn backend::ChatCompletionsBackend,
    http: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    request: &ChatRequest,
    out: Option<&Sender<String>>,
    render_to_terminal: bool,
    no_stream: bool,
    cancel: Option<&AtomicBool>,
    plain_terminal_stream: bool,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let t0 = Instant::now();
    let max_attempts = cfg.api_max_retries + 1;
    let mut last_ok = None;
    let mut req = request.clone();
    for attempt in 0..max_attempts {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(crate::types::LLM_CANCELLED_ERROR.into());
        }
        match llm_backend
            .stream_chat(
                http,
                api_key,
                &cfg.api_base,
                &mut req,
                out,
                render_to_terminal,
                no_stream,
                cancel,
                plain_terminal_stream,
            )
            .await
        {
            Ok(r) => {
                let (ref msg, ref finish_reason) = r;
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
                    crate::redact::assistant_message_preview_for_log(msg)
                );
                last_ok = Some(r);
                break;
            }
            Err(e) => {
                error!(
                    target: "crabmate",
                    "llm chat 请求失败 error={} attempt={} max_attempts={}",
                    e,
                    attempt + 1,
                    max_attempts
                );
                if attempt < max_attempts - 1 {
                    let delay_secs = cfg
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt));
                    info!(
                        target: "crabmate",
                        "llm 等待后重试 delay_secs={}",
                        delay_secs
                    );
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
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
            tool_calls: None,
            name: None,
            tool_call_id: None,
        };
        let messages = vec![Message::user_only("u"), sep, assistant];
        let a = super::no_tools_chat_request(&cfg, &messages, None, LlmSeedOverride::FromConfig);
        let stripped = messages_for_api_stripping_reasoning_skip_ui_separators(&messages);
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
}
