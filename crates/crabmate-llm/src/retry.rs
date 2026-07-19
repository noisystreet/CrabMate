//! 带指数退避的 `chat/completions` 重试入口（观测经 [`LlmRetryHooks`] 注入）。

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use crabmate_types::{ChatRequest, LlmConfig, Message};
use log::{debug, error, info};

use crate::backend::ChatCompletionsBackend;
use crate::call_error::{llm_call_error_http_status, llm_call_error_retryable};
use crate::chat_params::StreamChatParams;
use crate::complete_error::LlmCompleteError;
use crate::retry_hooks::{LlmRetryDecisionPoint, LlmRetryHooks};

static LLM_CALL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// [`super::complete_chat_retrying`] 入参（不含每次克隆前的 `ChatRequest`）。
pub struct CompleteChatRetryingParams<'a> {
    pub llm_backend: &'a dyn ChatCompletionsBackend,
    pub stream: StreamChatParams<'a>,
    pub cfg: &'a LlmConfig,
}

/// 调用 `chat/completions`：失败时若错误为 **可重试**，按配置指数退避；不可重试错误立即返回。
pub async fn complete_chat_retrying(
    p: &CompleteChatRetryingParams<'_>,
    hooks: &dyn LlmRetryHooks,
    request: &ChatRequest,
) -> Result<(Message, String), LlmCompleteError> {
    let llm_call_id = format!("llm-{}", LLM_CALL_SEQ.fetch_add(1, Ordering::Relaxed));
    let max_attempts = p.cfg.http_retry.api_max_retries + 1;
    hooks.append_turn_replay_json(
        "llm_request_started",
        request.model.as_str(),
        Some(serde_json::json!({
            "llm_call_id": llm_call_id,
            "model": request.model,
            "message_count": request.messages.len(),
            "max_attempts": max_attempts,
            "phase": "llm",
        })),
    );
    let t0 = Instant::now();
    let mut last_ok = None;
    let mut req = request.clone();
    for attempt in 0..max_attempts {
        let attempt_t0 = Instant::now();
        if p.stream.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            return Err(LlmCompleteError::Cancelled);
        }
        match p.llm_backend.stream_chat(&p.stream, &mut req).await {
            Ok(r) => {
                let (mut msg, finish_reason) = r;
                hooks.materialize_dsml_tool_calls(&mut msg);
                info!(
                    target: "crabmate",
                    "llm chat 完成 model={} elapsed_ms={} attempt={}",
                    request.model,
                    t0.elapsed().as_millis(),
                    attempt + 1
                );
                let preview = hooks.assistant_preview_for_log(&msg);
                debug!(
                    target: "crabmate",
                    "llm chat 响应摘要（含重试后成功） finish_reason={} message_in_request={} assistant_preview={}",
                    finish_reason,
                    request.messages.len(),
                    preview
                );
                hooks.append_turn_replay_json(
                    "llm_response_done",
                    request.model.as_str(),
                    Some(serde_json::json!({
                        "llm_call_id": llm_call_id,
                        "model": request.model,
                        "finish_reason": finish_reason,
                        "attempt": attempt + 1,
                        "max_attempts": max_attempts,
                        "attempt_elapsed_ms": attempt_t0.elapsed().as_millis(),
                        "total_elapsed_ms": t0.elapsed().as_millis(),
                        "assistant_preview": preview,
                        "assistant_preview_truncated": preview.contains("…(truncated)"),
                        "assistant_content": hooks.assistant_content_for_log(&msg),
                        "reasoning_content": hooks.reasoning_content_for_log(&msg),
                        "phase": "llm",
                        "tool_calls": msg.tool_calls.as_ref().map(|calls| {
                            calls.iter().map(|tc| {
                                let args_preview =
                                    hooks.tool_arguments_preview_for_sse(&tc.function.arguments);
                                serde_json::json!({
                                    "id": tc.id,
                                    "name": tc.function.name,
                                    "arguments_preview": args_preview,
                                    "arguments_preview_truncated": args_preview.contains("…(truncated)"),
                                })
                            }).collect::<Vec<_>>()
                        }).unwrap_or_default(),
                    })),
                );
                last_ok = Some((msg, finish_reason));
                break;
            }
            Err(e) => {
                let http_status = llm_call_error_http_status(e.as_ref());
                let retryable = llm_call_error_retryable(e.as_ref());
                let can_backoff = attempt < max_attempts - 1 && retryable;
                let backoff_ms = if can_backoff {
                    p.cfg
                        .http_retry
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
                hooks.append_turn_replay_json(
                    "llm_request_failed",
                    request.model.as_str(),
                    Some(serde_json::json!({
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
                hooks.append_decision_point(&LlmRetryDecisionPoint {
                    phase: "llm".to_string(),
                    decision_id: "llm_retry_policy".to_string(),
                    outcome: if can_backoff {
                        "retry".to_string()
                    } else {
                        "stop".to_string()
                    },
                    rationale: if can_backoff {
                        "请求失败且可重试，按指数退避继续重试".to_string()
                    } else if retryable {
                        "达到最大重试次数，停止重试".to_string()
                    } else {
                        "错误不可重试，立即停止".to_string()
                    },
                    detail: serde_json::json!({
                        "llm_call_id": llm_call_id,
                        "attempt": attempt + 1,
                        "max_attempts": max_attempts,
                        "retryable": retryable,
                        "http_status": http_status,
                        "backoff_ms": backoff_ms,
                    }),
                    anchor_kind: "current_llm_call".to_string(),
                    anchor: Some(serde_json::json!({
                        "llm_call_id": llm_call_id,
                    })),
                });
                if can_backoff {
                    let delay_secs = p
                        .cfg
                        .http_retry
                        .api_retry_delay_secs
                        .saturating_mul(2_u64.saturating_pow(attempt));
                    info!(
                        target: "crabmate",
                        "llm 等待后重试 delay_secs={}",
                        delay_secs
                    );
                    tokio::time::sleep(Duration::from_secs(delay_secs)).await;
                    if p.stream.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
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
