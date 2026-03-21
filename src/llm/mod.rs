//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的封装层。
//!
//! - **`api`**：单次 HTTP + SSE/JSON 解析 + 可选终端 Markdown 渲染（传输与协议细节）。
//! - **本模块**：`ChatRequest` 的惯用构造、带指数退避的**重试策略**、以及后续可扩展的调用入口（例如统一超时、观测字段）。
//!
//! Agent 主循环应通过 [`complete_chat_retrying`] 发请求，避免在 `agent::agent_turn` 中散落重试与请求拼装逻辑。

mod api;

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::Sender;
use tracing::{error, info};

use crate::config::AgentConfig;
use crate::types::{ChatRequest, Message, Tool};
use api::stream_chat;
use reqwest::Client;

/// 构造带 tools、**`tool_choice: auto`** 及采样参数的请求体（`stream` 由 [`api::stream_chat`] 按 `no_stream` 覆盖）。
pub fn tool_chat_request(cfg: &AgentConfig, messages: &[Message], tools: &[Tool]) -> ChatRequest {
    ChatRequest {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
        tools: Some(tools.to_vec()),
        tool_choice: Some("auto".to_string()),
        max_tokens: cfg.max_tokens,
        temperature: cfg.temperature,
        stream: None,
    }
}

/// 构造**无 tools** 的请求（`tool_choice` 省略），用于分阶段规划首轮等场景。
pub fn nl_only_chat_request(cfg: &AgentConfig, messages: &[Message]) -> ChatRequest {
    ChatRequest {
        model: cfg.model.clone(),
        messages: messages.to_vec(),
        tools: None,
        tool_choice: None,
        max_tokens: cfg.max_tokens,
        temperature: cfg.temperature,
        stream: None,
    }
}

/// 调用 `chat/completions`：失败时按 `AgentConfig::api_retry_delay_secs` 做指数退避，最多 `api_max_retries + 1` 次。
#[allow(clippy::too_many_arguments)]
pub async fn complete_chat_retrying(
    http: &Client,
    api_key: &str,
    cfg: &AgentConfig,
    request: &ChatRequest,
    out: Option<&Sender<String>>,
    render_to_terminal: bool,
    no_stream: bool,
    cancel: Option<&AtomicBool>,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let t0 = Instant::now();
    let max_attempts = cfg.api_max_retries + 1;
    let mut last_ok = None;
    for attempt in 0..max_attempts {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(crate::types::LLM_CANCELLED_ERROR.into());
        }
        match stream_chat(
            http,
            api_key,
            &cfg.api_base,
            request,
            out,
            render_to_terminal,
            no_stream,
            cancel,
        )
        .await
        {
            Ok(r) => {
                info!(
                    model = %request.model,
                    elapsed_ms = t0.elapsed().as_millis(),
                    attempt = attempt + 1,
                    "llm chat 完成"
                );
                last_ok = Some(r);
                break;
            }
            Err(e) => {
                error!(
                    error = %e,
                    attempt = attempt + 1,
                    max_attempts = max_attempts,
                    "llm chat 请求失败"
                );
                if attempt < max_attempts - 1 {
                    let delay_secs = cfg
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt));
                    info!(delay_secs = delay_secs, "llm 等待后重试");
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
    use crate::types::OPENAI_CHAT_COMPLETIONS_REL_PATH;

    #[test]
    fn completions_path_matches_openai_compat() {
        assert_eq!(OPENAI_CHAT_COMPLETIONS_REL_PATH, "chat/completions");
    }
}
