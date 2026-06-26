//! 与大模型（OpenAI 兼容 **`/chat/completions`**）交互的封装层。
//!
//! - **`api`**：单次 HTTP + SSE/JSON 解析 + 可选终端 Markdown 渲染（传输与协议细节）。
//! - **`backend`**：[`ChatCompletionsBackend`] 可插拔抽象，默认 [`OpenAiCompatBackend`]（即 `api::stream_chat`）。
//! - **`vendor`**：按网关族调整出站 JSON（温度、`thinking`、`reasoning_effort`、是否保留 tool 轮 `reasoning_content`）；新增厂商见 [`vendor::LlmVendorAdapter`]。
//! - **本模块**：带指数退避的**重试策略**（仅对 [`call_error::LlmCallError`] 标记为 `retryable` 的失败：如 **408/429/5xx** 与部分传输错误；**401/400** 等客户端错误不重试）；[`complete_chat_retrying`] 失败为 [`LlmCompleteError`]（**无**「第几步规划」等编排语义）；以及后续可扩展的调用入口（例如统一超时、观测字段）。
//!
//! `ChatRequest` 惯用构造与供应商出站 `messages` 变换见 **`crabmate_llm`**（[`tool_chat_request`]、[`conversation_messages_to_vendor_body`] 等），经本模块再导出以保持 `crate::llm::` 路径稳定。
//!
//! Agent 主循环应通过 [`complete_chat_retrying`] 发请求，避免在 `agent::agent_turn` 中散落重试与请求拼装逻辑。
//! **维护约定**（唯一入口 / 禁止直接 `api::stream_chat`）见 **`docs/开发文档.md`** 章节「`agent_turn` 与 `llm`：唯一入口与禁止事项」。

mod api;
mod backend_openai;
mod chat_params_ext;

pub mod backend {
    pub use super::backend_openai::{
        OPENAI_COMPAT_BACKEND, OpenAiCompatBackend, default_chat_completions_backend,
    };
    pub use crabmate_llm::backend::ChatCompletionsBackend;
}

pub use backend::{
    ChatCompletionsBackend, OPENAI_COMPAT_BACKEND, OpenAiCompatBackend,
    default_chat_completions_backend,
};
pub use chat_params_ext::CompleteChatRetryingParams;
#[allow(unused_imports)]
pub use crabmate_llm::{
    LlmCallError, LlmCompleteError, LlmRetryingTransportOpts, LlmVendorAdapter, StreamChatParams,
    TuiLlmStreamScratchArc, chat_request_vendor_extensions_for_agent,
    conversation_messages_to_vendor_body, fetch_models_report, fold_system_into_user_for_config,
    kimi_k2_5_vendor_requires_tool_call_reasoning, llm_vendor_adapter,
    llm_vendor_adapter_for_model, no_tools_chat_request,
    no_tools_chat_request_for_hierarchical_manager, no_tools_chat_request_from_messages,
    normalize_stripped_messages_for_vendor_body, tool_chat_request, vendor,
    vendor_temperature_for_config, vendor_temperature_for_model,
};

pub(crate) use crabmate_llm::STAGED_PLANNER_MIN_COMPLETION_TOKENS;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use log::{debug, error, info};

use crate::types::{ChatRequest, Message};

static LLM_CALL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// 调用 `chat/completions`：失败时若错误为 **可重试**（见 [`call_error::LlmCallError`]），按 `AgentConfig::api_retry_delay_secs` 做指数退避，最多 `api_max_retries + 1` 次；**401/400** 等不可重试错误立即返回。
///
/// `llm_backend` 默认使用 [`default_chat_completions_backend`]（OpenAI 兼容 HTTP）；可换为自定义 [`ChatCompletionsBackend`]。
pub async fn complete_chat_retrying(
    p: &CompleteChatRetryingParams<'_>,
    request: &ChatRequest,
) -> Result<(Message, String), LlmCompleteError> {
    let llm_call_id = format!("llm-{}", LLM_CALL_SEQ.fetch_add(1, Ordering::Relaxed));
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "llm_request_started",
        request.model.as_str(),
        Some(&serde_json::json!({
            "llm_call_id": llm_call_id,
            "model": request.model,
            "message_count": request.messages.len(),
            "max_attempts": p.cfg.llm_http_retry.api_max_retries + 1,
            "phase": "llm",
        })),
    );
    let _llm_trace = p
        .request_chrome_trace
        .as_ref()
        .map(|t| t.enter_section("llm.chat_completions"));
    let t0 = Instant::now();
    let max_attempts = p.cfg.llm_http_retry.api_max_retries + 1;
    let mut last_ok = None;
    let mut req = request.clone();
    let stream = p.stream_params();
    for attempt in 0..max_attempts {
        let attempt_t0 = Instant::now();
        if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(LlmCompleteError::Cancelled);
        }
        match p.llm_backend.stream_chat(&stream, &mut req).await {
            Ok(r) => {
                let (mut msg, finish_reason) = r;
                crate::dsml::materialize_deepseek_dsml_tool_calls_in_message(
                    &mut msg,
                    p.cfg.dsml_materialize.materialize_deepseek_dsml_tool_calls,
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
                crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
                    "llm_response_done",
                    request.model.as_str(),
                    Some(&serde_json::json!({
                        "llm_call_id": llm_call_id,
                        "model": request.model,
                        "finish_reason": finish_reason,
                        "attempt": attempt + 1,
                        "max_attempts": max_attempts,
                        "attempt_elapsed_ms": attempt_t0.elapsed().as_millis(),
                        "total_elapsed_ms": t0.elapsed().as_millis(),
                        "assistant_preview": crate::redact::assistant_message_preview_for_log(&msg),
                        "assistant_preview_truncated": crate::redact::assistant_message_preview_for_log(&msg).contains("…(truncated)"),
                        "assistant_content": crate::types::message_content_as_str(&msg.content).unwrap_or(""),
                        "reasoning_content": msg.reasoning_content.as_deref().unwrap_or(""),
                        "phase": "llm",
                        "tool_calls": msg.tool_calls.as_ref().map(|calls| {
                            calls.iter().map(|tc| serde_json::json!({
                                "id": tc.id,
                                "name": tc.function.name,
                                "arguments_preview": crate::redact::tool_arguments_preview_for_sse(&tc.function.arguments),
                                "arguments_preview_truncated": crate::redact::tool_arguments_preview_for_sse(&tc.function.arguments).contains("…(truncated)"),
                            })).collect::<Vec<_>>()
                        }).unwrap_or_default(),
                    })),
                );
                last_ok = Some((msg, finish_reason));
                break;
            }
            Err(e) => {
                let http_status = crabmate_llm::call_error::llm_call_error_http_status(e.as_ref());
                let retryable = crabmate_llm::call_error::llm_call_error_retryable(e.as_ref());
                let can_backoff = attempt < max_attempts - 1 && retryable;
                let backoff_ms = if can_backoff {
                    p.cfg
                        .llm_http_retry
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt))
                        .saturating_mul(1000)
                } else {
                    0
                };
                error!(
                    target: "crabmate",
                    "llm chat 请求失败 http_status={:?} retryable={} error={} attempt={} max_attempts={}",
                    http_status,
                    retryable,
                    e,
                    attempt + 1,
                    max_attempts
                );
                crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
                    "llm_request_failed",
                    request.model.as_str(),
                    Some(&serde_json::json!({
                        "llm_call_id": llm_call_id,
                        "model": request.model,
                        "attempt": attempt + 1,
                        "max_attempts": max_attempts,
                        "attempt_elapsed_ms": attempt_t0.elapsed().as_millis(),
                        "total_elapsed_ms": t0.elapsed().as_millis(),
                        "retryable": retryable,
                        "http_status": http_status,
                        "error": format!("{e}"),
                        "will_retry": can_backoff,
                        "retry_reason": if retryable { "retryable_error" } else { "non_retryable_error" },
                        "backoff_ms": backoff_ms,
                        "phase": "llm",
                    })),
                );
                crate::turn_replay_dump::append_decision_point_event_if_configured(
                    "llm",
                    "llm_retry_policy",
                    if can_backoff { "retry" } else { "stop" },
                    if can_backoff {
                        "请求失败且可重试，按指数退避继续重试"
                    } else if retryable {
                        "达到最大重试次数，停止重试"
                    } else {
                        "错误不可重试，立即停止"
                    },
                    serde_json::json!({
                        "llm_call_id": llm_call_id,
                        "attempt": attempt + 1,
                        "max_attempts": max_attempts,
                        "retryable": retryable,
                        "http_status": http_status,
                        "backoff_ms": backoff_ms,
                    }),
                    "current_llm_call",
                    Some(serde_json::json!({
                        "llm_call_id": llm_call_id,
                    })),
                );
                if can_backoff {
                    let delay_secs = p
                        .cfg
                        .llm_http_retry
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt));
                    info!(
                        target: "crabmate",
                        "llm 等待后重试 delay_secs={}",
                        delay_secs
                    );
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    if p.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                        return Err(LlmCompleteError::Cancelled);
                    }
                } else {
                    return Err(LlmCompleteError::from_boxed(e));
                }
            }
        }
    }
    last_ok.ok_or_else(|| {
        LlmCompleteError::Other(
            std::io::Error::other("llm chat 成功分支未写入结果（逻辑错误）").into(),
        )
    })
}
