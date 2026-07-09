//! OpenAI 兼容 **`chat/completions`** 的单次 HTTP 调用：SSE/JSON 解析与可选终端输出（经 [`StreamChatHost`] 注入）。

mod error_handler;
mod sse_parser;
mod sse_turn_segment_emit;

use log::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};

use crabmate_config::LlmHttpAuthMode;
use crabmate_sse_protocol::StreamEndReason;
use crabmate_types::{
    ChatRequest, FunctionCall, LLM_CANCELLED_ERROR, Message, MessageContent,
    OPENAI_CHAT_COMPLETIONS_REL_PATH, ToolCall, USER_CANCELLED_FINISH_REASON, Usage,
    merge_reasoning_details_into_reasoning_content, message_content_as_str,
    message_content_byte_len_for_estimate, sanitize_tool_call_arguments_for_openai_compat,
};

use crate::call_error::LlmCallError;
use crate::chat_params::StreamChatParams;
use crate::stream_host::StreamChatHost;
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
                    arguments: sanitize_tool_call_arguments_for_openai_compat(&arguments),
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
        target: "crabmate_llm",
        "prompt_cache model={} hit={} miss={} ratio={:.1}%",
        model,
        hit,
        miss,
        ratio * 100.0
    );
    crate::cache_stats::LLM_CACHE_AGGREGATE.record(u);
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
    plain_terminal_stream: bool,
    cancel: Option<&AtomicBool>,
) {
    if plain_terminal_stream {
        let sse_plain = host.assistant_streaming_plain_concat(msg);
        if !sse_plain.is_empty() {
            let _ = sse_out_send(
                host,
                tx,
                sse_plain,
                "llm::stream_chat non-stream assistant plain",
                cancel,
            )
            .await;
        }
    } else {
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

#[allow(clippy::too_many_arguments)]
async fn non_stream_chat_response(
    host: &dyn StreamChatHost,
    res: reqwest::Response,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    render_to_terminal: bool,
    plain_terminal_stream: bool,
    cancel: Option<&AtomicBool>,
    cli_terminal_plain: bool,
    model: &str,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let _cli_wait_spinner = host.try_start_cli_wait_spinner(cli_terminal_plain);
    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        return Err(LLM_CANCELLED_ERROR.into());
    }
    let body = res.text().await.map_err(LlmCallError::boxed_from_reqwest)?;
    let parsed: crabmate_types::ChatResponse = serde_json::from_str(&body)
        .map_err(|e| boxed_non_stream_chat_parse_error(host, &body, &e))?;
    let usage = parsed.usage;
    let choice = parsed.choices.into_iter().next().ok_or_else(
        || -> Box<dyn std::error::Error + Send + Sync> { "非流式响应 choices 为空".into() },
    )?;
    let crabmate_types::Choice {
        message: mut msg,
        finish_reason,
    } = choice;

    merge_reasoning_details_into_reasoning_content(&mut msg);

    if let Some(tx) = out {
        non_stream_emit_sse_for_assistant(host, &msg, tx, plain_terminal_stream, cancel).await;
    }
    if render_to_terminal {
        host.render_non_stream_assistant_terminal(&msg, plain_terminal_stream, out.is_none())?;
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
    if render_to_terminal && out.is_none() {
        host.print_stream_end_reason_terminal(terminal_end_reason);
    }
    log_cache_usage(usage.as_ref(), model);
    Ok((msg, finish_reason))
}

async fn streaming_chat_response(
    host: &dyn StreamChatHost,
    res: reqwest::Response,
    params: &StreamChatParams<'_>,
    render_to_terminal: bool,
    model: &str,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let cli_terminal_plain =
        render_to_terminal && params.out.is_none() && params.plain_terminal_stream;
    let _cli_wait_spinner = host.try_start_cli_wait_spinner(cli_terminal_plain);
    let stream = res.bytes_stream();
    let acc = consume_openai_sse_byte_stream(
        host,
        stream,
        ConsumeSseStreamOpts {
            cancel: params.cancel,
            out: params.out,
            cli_terminal_plain,
            thinking_trace_enabled: params.thinking_trace_enabled,
            dsml_stream_strip_enabled: params.dsml_stream_strip_enabled,
            tui_llm_stream_scratch: params.tui_llm_stream_scratch.clone(),
        },
    )
    .await?;

    if render_to_terminal {
        let md = host.assistant_raw_markdown_body_from_parts(
            acc.reasoning_acc.as_str(),
            acc.content_acc.as_str(),
        );
        if cli_terminal_plain {
            host.finalize_stream_plain_terminal_suffix(
                acc.cli_plain_reasoning_style_active,
                acc.cli_plain_prefix_emitted,
                acc.content_acc.as_str(),
                acc.reasoning_acc.as_str(),
            )?;
        } else if !md.is_empty() {
            host.terminal_render_agent_markdown(&md)?;
        }
    }

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
    if render_to_terminal && params.out.is_none() {
        host.print_stream_end_reason_terminal(terminal_end_reason);
    }
    log_cache_usage(usage.as_ref(), model);
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
        render_to_terminal,
        no_stream,
        cancel,
        plain_terminal_stream,
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
    req.messages = crate::vendor_messages::conversation_messages_to_vendor_body(
        &taken,
        fold_system_into_user,
        preserve_reasoning_on_assistant_tool_calls,
        preserve_deepseek_thinking_reasoning_roundtrip,
    );
    log_chat_request_json_preview_if_enabled(host, req);
    req.stream = Some(!no_stream);

    // 序列化为 JSON，条件注入 cache_control（DeepSeek 等供应商支持）
    let mut body = serde_json::to_value(&req)
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;
    if api_base.to_ascii_lowercase().contains("deepseek") {
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

    let cli_terminal_plain = render_to_terminal && out.is_none() && plain_terminal_stream;

    let model = req.model.clone();
    if no_stream {
        non_stream_chat_response(
            host,
            res,
            out,
            render_to_terminal,
            plain_terminal_stream,
            cancel,
            cli_terminal_plain,
            &model,
        )
        .await
    } else {
        streaming_chat_response(host, res, params, render_to_terminal, &model).await
    }
}
