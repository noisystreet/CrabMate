//! OpenAI 兼容 **`chat/completions`** 的单次 HTTP 调用：SSE/JSON 解析（经 [`StreamChatHost`] 注入侧效应）。

mod error_handler;
mod sse_parser;
mod sse_turn_segment_emit;

use log::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::cm_sse_protocol::StreamEndReason;
use crate::cm_types::llm_config::LlmHttpAuthMode;
use crate::cm_types::{
    ChatRequest, FunctionCall, LLM_CANCELLED_ERROR, Message, MessageContent,
    OPENAI_CHAT_COMPLETIONS_REL_PATH, ToolCall, USER_CANCELLED_FINISH_REASON, Usage,
    merge_reasoning_details_into_reasoning_content, message_content_as_str,
    message_content_byte_len_for_estimate, prepare_tool_call_arguments_for_local_execution,
};

use crate::cm_llm::call_error::LlmCallError;
use crate::cm_llm::chat_params::StreamChatParams;
use crate::cm_llm::stream_host::StreamChatHost;
use error_handler::{
    boxed_non_stream_chat_parse_error, ensure_chat_completions_success,
    log_chat_request_json_preview_if_enabled,
};
use sse_parser::{
    ConsumeSseStreamOpts, SseStreamAccum, consume_openai_sse_byte_stream, sse_out_send,
};

fn tool_calls_from_sse_accum(
    tool_calls_acc: Vec<(String, String, String, String)>,
) -> Option<Vec<ToolCall>> {
    if tool_calls_acc.is_empty() {
        return None;
    }
    Some(
        tool_calls_acc
            .into_iter()
            .map(|(id, typ, name, arguments)| ToolCall {
                id,
                typ,
                function: FunctionCall {
                    name,
                    // 本地执行：非法 JSON 保留原文，避免洗成 `{}` 后误报 Schema 缺 path。
                    // 发往上游仍由 `conversation_messages_to_vendor_body` 做 `{}` 回退。
                    arguments: prepare_tool_call_arguments_for_local_execution(&arguments),
                },
            })
            .collect(),
    )
}

/// 记录缓存命中统计并累积到进程级单例。
fn log_cache_usage(usage: Option<&Usage>, model: &str) {
    let Some(u) = usage else { return };
    let hit = u.prompt_cache_hit_tokens.unwrap_or(0);
    let miss = u.prompt_cache_miss_tokens.unwrap_or(0);
    let total = hit + miss;
    let ratio = if total > 0 {
        hit as f64 / total as f64
    } else {
        0.0
    };
    log::info!(
        target: "crate::cm_llm",
        "prompt_cache model={} hit={} miss={} ratio={:.1}%",
        model,
        hit,
        miss,
        ratio * 100.0
    );
    crate::cm_llm::cache_stats::LLM_CACHE_AGGREGATE.record(u);
}

/// 在序列化后的请求 JSON 中对 system 消息注入 `cache_control: {"type": "ephemeral"}`。
/// 不修改 `Message` 类型，通过 JSON 后处理实现最小侵入。
fn inject_cache_control_json(mut body: serde_json::Value) -> serde_json::Value {
    let Some(messages) = body.get_mut("messages").and_then(|m| m.as_array_mut()) else {
        return body;
    };
    for msg in messages.iter_mut() {
        if msg.get("role").and_then(|r| r.as_str()) != Some("system") {
            continue;
        }
        if let Some(obj) = msg.as_object_mut() {
            obj.insert(
                "cache_control".to_string(),
                serde_json::json!({"type": "ephemeral"}),
            );
        }
    }
    body
}

fn message_from_sse_accum(acc: SseStreamAccum) -> Message {
    let SseStreamAccum {
        reasoning_acc,
        content_acc,
        tool_calls_acc,
        ..
    } = acc;
    Message {
        role: "assistant".to_string(),
        content: if content_acc.is_empty() {
            None
        } else {
            Some(MessageContent::Text(content_acc))
        },
        reasoning_content: if reasoning_acc.is_empty() {
            None
        } else {
            Some(reasoning_acc)
        },
        reasoning_details: None,
        tool_calls: tool_calls_from_sse_accum(tool_calls_acc),
        name: None,
        tool_call_id: None,
    }
}

fn stream_end_reason_from_finish_and_message(
    finish_reason: &str,
    msg: &Message,
) -> StreamEndReason {
    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return StreamEndReason::Cancelled;
    }
    let has_content = message_content_as_str(&msg.content)
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    let has_reasoning = msg
        .reasoning_content
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty());
    if has_content || has_reasoning {
        StreamEndReason::Completed
    } else {
        StreamEndReason::NoOutput
    }
}

async fn non_stream_emit_sse_for_assistant(
    host: &dyn StreamChatHost,
    msg: &Message,
    tx: &tokio::sync::mpsc::Sender<String>,
    cancel: Option<&AtomicBool>,
) {
    let r = msg.reasoning_content.as_deref().unwrap_or("");
    let c = message_content_as_str(&msg.content).unwrap_or("");
    if !r.is_empty() {
        let _ = sse_out_send(
            host,
            tx,
            r.to_string(),
            "llm::stream_chat non-stream assistant reasoning",
            cancel,
        )
        .await;
    }
    if !c.is_empty() {
        let _ = sse_out_send(
            host,
            tx,
            host.encode_assistant_answer_phase_sse(),
            "llm::stream_chat non-stream assistant_answer_phase",
            cancel,
        )
        .await;
        let _ = sse_out_send(
            host,
            tx,
            c.to_string(),
            "llm::stream_chat non-stream assistant content",
            cancel,
        )
        .await;
    }
}

async fn non_stream_emit_parsing_tool_calls_if_needed(
    host: &dyn StreamChatHost,
    msg: &Message,
    tx: &tokio::sync::mpsc::Sender<String>,
    cancel: Option<&AtomicBool>,
) {
    if msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
        let _ = sse_out_send(
            host,
            tx,
            host.encode_parsing_tool_calls_sse(),
            "llm::stream_chat non-stream parsing_tool_calls",
            cancel,
        )
        .await;
    }
}

async fn non_stream_chat_response(
    host: &dyn StreamChatHost,
    res: reqwest::Response,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    cancel: Option<&AtomicBool>,
    model: &str,
    provider_usage_sink: Option<
        &std::sync::Arc<std::sync::Mutex<Option<crate::cm_types::Usage>>>,
    >,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        return Err(LLM_CANCELLED_ERROR.into());
    }
    let body = res.text().await.map_err(LlmCallError::boxed_from_reqwest)?;
    let parsed: crate::cm_types::ChatResponse = serde_json::from_str(&body)
        .map_err(|e| boxed_non_stream_chat_parse_error(host, &body, &e))?;
    let usage = parsed.usage;
    let choice = parsed.choices.into_iter().next().ok_or_else(
        || -> Box<dyn std::error::Error + Send + Sync> { "非流式响应 choices 为空".into() },
    )?;
    let crate::cm_types::Choice {
        message: mut msg,
        finish_reason,
    } = choice;

    merge_reasoning_details_into_reasoning_content(&mut msg);

    if let Some(tx) = out {
        non_stream_emit_sse_for_assistant(host, &msg, tx, cancel).await;
    }
    if let Some(tx) = out {
        non_stream_emit_parsing_tool_calls_if_needed(host, &msg, tx, cancel).await;
    }
    debug!(
        target: "crabmate",
        "chat completions 非流式响应 finish_reason={} content_len={} tool_calls={} assistant_preview={}",
        finish_reason,
        message_content_byte_len_for_estimate(&msg.content),
        msg.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        host.assistant_message_preview_for_log(&msg)
    );
    let terminal_end_reason = stream_end_reason_from_finish_and_message(&finish_reason, &msg);
    host.append_stream_diagnostic_event(terminal_end_reason.as_str(), &msg);
    log_cache_usage(usage.as_ref(), model);
    record_provider_usage(provider_usage_sink, usage);
    Ok((msg, finish_reason))
}

fn record_provider_usage(
    sink: Option<&std::sync::Arc<std::sync::Mutex<Option<crate::cm_types::Usage>>>>,
    usage: Option<crate::cm_types::Usage>,
) {
    let (Some(sink), Some(usage)) = (sink, usage) else {
        return;
    };
    if let Ok(mut slot) = sink.lock() {
        *slot = Some(usage);
    }
}

async fn streaming_chat_response(
    host: &dyn StreamChatHost,
    res: reqwest::Response,
    params: &StreamChatParams<'_>,
    model: &str,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let stream = res.bytes_stream();
    let acc = consume_openai_sse_byte_stream(
        host,
        stream,
        ConsumeSseStreamOpts {
            cancel: params.cancel,
            out: params.out,
            thinking_trace_enabled: params.thinking_trace_enabled,
        },
    )
    .await?;

    let finish = if params.cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        USER_CANCELLED_FINISH_REASON.to_string()
    } else {
        acc.finish_reason.clone()
    };
    let usage = acc.usage;
    let msg = message_from_sse_accum(acc);
    debug!(
        target: "crabmate",
        "chat completions 流式响应拼装完成 finish_reason={} content_len={} tool_calls={} assistant_preview={}",
        finish,
        message_content_byte_len_for_estimate(&msg.content),
        msg.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        host.assistant_message_preview_for_log(&msg)
    );
    let terminal_end_reason = stream_end_reason_from_finish_and_message(&finish, &msg);
    host.append_stream_diagnostic_event(terminal_end_reason.as_str(), &msg);
    log_cache_usage(usage.as_ref(), model);
    record_provider_usage(params.provider_usage_sink, usage);
    Ok((msg, finish))
}

/// 请求 chat/completions：`no_stream == false` 时为 SSE 流式；`true` 时为单次 JSON（`stream: false`）。
pub async fn stream_chat(
    params: &StreamChatParams<'_>,
    req: &mut ChatRequest,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let StreamChatParams {
        host,
        client,
        api_key,
        api_base,
        auth_mode,
        out,
        no_stream,
        cancel,
        fold_system_into_user,
        preserve_reasoning_on_assistant_tool_calls,
        preserve_deepseek_thinking_reasoning_roundtrip,
        ..
    } = *params;

    let url = format!(
        "{}/{}",
        api_base.trim_end_matches('/'),
        OPENAI_CHAT_COMPLETIONS_REL_PATH
    );
    info!(
        target: "crabmate",
        "发起 chat 请求 url={} model={} streaming={}",
        url,
        req.model,
        !no_stream
    );

    let taken = std::mem::take(&mut req.messages);
    req.messages = crate::cm_llm::vendor_messages::conversation_messages_to_vendor_body(
        &taken,
        fold_system_into_user,
        preserve_reasoning_on_assistant_tool_calls,
        preserve_deepseek_thinking_reasoning_roundtrip,
    );
    log_chat_request_json_preview_if_enabled(host, req);
    let model = req.model.clone();
    let api_base_owned = api_base.to_string();
    let uploads = params.chat_uploads_dir.map(std::path::Path::to_path_buf);
    let workspace = params
        .chat_workspace_root
        .map(std::path::Path::to_path_buf);
    let mut messages = std::mem::take(&mut req.messages);
    req.messages = tokio::task::spawn_blocking(move || {
        crate::cm_llm::outbound_images::rewrite_messages_for_vendor(
            &mut messages,
            &model,
            &api_base_owned,
            uploads.as_deref(),
            workspace.as_deref(),
        );
        messages
    })
    .await
    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
    req.stream = Some(!no_stream);

    // 序列化为 JSON，条件注入 cache_control（DeepSeek 等供应商支持）
    let mut body = serde_json::to_value(&req)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
    if crate::cm_llm::vendor_catalog::resolved_vendor_caps(&req.model, api_base)
        .explicit_cache_control
    {
        body = inject_cache_control_json(body);
    }

    let mut rb = client.post(&url).json(&body);
    if auth_mode == LlmHttpAuthMode::Bearer {
        rb = rb.header("Authorization", format!("Bearer {}", api_key));
    }
    let res = rb.send().await.map_err(LlmCallError::boxed_from_reqwest)?;
    let res = ensure_chat_completions_success(host, res)
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    let model = req.model.clone();
    if no_stream {
        non_stream_chat_response(
            host,
            res,
            out,
            cancel,
            &model,
            params.provider_usage_sink,
        )
        .await
    } else {
        streaming_chat_response(host, res, params, &model).await
    }
}
