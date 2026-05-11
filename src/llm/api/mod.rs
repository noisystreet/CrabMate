//! OpenAI 兼容 **`chat/completions`** 的单次 HTTP 调用：SSE/JSON 解析、终端 Markdown 与 LaTeX→Unicode。
//!
//! 子模块：[`error_handler`]（HTTP/JSON 错误与请求体日志）、[`sse_parser`]（SSE 行协议与 delta 累积）、[`terminal_render`]（CLI 输出）。
//! 带 **tools** 的 `ChatRequest` 构造、**退避重试**与 Agent 侧调用入口见上级 [`super`]（`llm`）。

mod error_handler;
mod sse_parser;
mod terminal_render;

pub use terminal_render::terminal_render_agent_markdown;

use log::{debug, info};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::config::LlmHttpAuthMode;
use crate::redact;
use crate::types::{
    ChatRequest, FunctionCall, Message, MessageContent, ToolCall, USER_CANCELLED_FINISH_REASON,
    message_content_byte_len_for_estimate,
};
use crabmate_sse_protocol::StreamEndReason;

use super::call_error::LlmCallError;
use error_handler::{
    boxed_non_stream_chat_parse_error, ensure_chat_completions_success,
    log_chat_request_json_preview_if_enabled,
};
use sse_parser::{SseStreamAccum, consume_openai_sse_byte_stream, sse_out_send};
use terminal_render::{
    finalize_stream_plain_terminal_suffix, render_non_stream_assistant_terminal,
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
                    arguments: crate::types::sanitize_tool_call_arguments_for_openai_compat(
                        &arguments,
                    ),
                },
            })
            .collect(),
    )
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

fn append_stream_diagnostic_event(stream_ended: &str, msg: &Message) {
    let answer_text = crate::types::message_content_as_str(&msg.content).unwrap_or("");
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "stream_diagnostic",
        "final_stream_status",
        Some(&serde_json::json!({
            "phase": "stream_diagnostic",
            "stream_ended": stream_ended,
            "answer_phase": !answer_text.is_empty(),
            "delta_chars": answer_text.chars().count(),
        })),
    );
}

fn stream_end_reason_from_finish_and_message(
    finish_reason: &str,
    msg: &Message,
) -> StreamEndReason {
    if finish_reason == USER_CANCELLED_FINISH_REASON {
        return StreamEndReason::Cancelled;
    }
    let has_content = crate::types::message_content_as_str(&msg.content)
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
    msg: &Message,
    tx: &tokio::sync::mpsc::Sender<String>,
    plain_terminal_stream: bool,
    cancel: Option<&AtomicBool>,
) {
    if plain_terminal_stream {
        let sse_plain = crate::runtime::message_display::assistant_streaming_plain_concat(msg);
        if !sse_plain.is_empty() {
            let _ = sse_out_send(
                tx,
                sse_plain,
                "llm::stream_chat non-stream assistant plain",
                cancel,
            )
            .await;
        }
    } else {
        let r = msg.reasoning_content.as_deref().unwrap_or("");
        let c = crate::types::message_content_as_str(&msg.content).unwrap_or("");
        if !r.is_empty() {
            let _ = sse_out_send(
                tx,
                r.to_string(),
                "llm::stream_chat non-stream assistant reasoning",
                cancel,
            )
            .await;
        }
        if !c.is_empty() {
            let _ = sse_out_send(
                tx,
                crate::sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
                    assistant_answer_phase: true,
                }),
                "llm::stream_chat non-stream assistant_answer_phase",
                cancel,
            )
            .await;
            let _ = sse_out_send(
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
    msg: &Message,
    tx: &tokio::sync::mpsc::Sender<String>,
    cancel: Option<&AtomicBool>,
) {
    if msg.tool_calls.as_ref().is_some_and(|t| !t.is_empty()) {
        let _ = sse_out_send(
            tx,
            crate::sse::encode_message(crate::sse::SsePayload::ParsingToolCalls {
                parsing_tool_calls: true,
            }),
            "llm::stream_chat non-stream parsing_tool_calls",
            cancel,
        )
        .await;
    }
}

async fn non_stream_chat_response(
    res: reqwest::Response,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    render_to_terminal: bool,
    plain_terminal_stream: bool,
    cancel: Option<&AtomicBool>,
    cli_terminal_plain: bool,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let _cli_wait_spinner =
        crate::runtime::cli_wait_spinner::CliWaitSpinnerGuard::try_start_for_cli_plain_stream(
            cli_terminal_plain,
        );
    if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        return Err(crate::types::LLM_CANCELLED_ERROR.into());
    }
    let body = res.text().await.map_err(LlmCallError::boxed_from_reqwest)?;
    let parsed: crate::types::ChatResponse =
        serde_json::from_str(&body).map_err(|e| boxed_non_stream_chat_parse_error(&body, &e))?;
    let choice = parsed.choices.into_iter().next().ok_or_else(
        || -> Box<dyn std::error::Error + Send + Sync> { "非流式响应 choices 为空".into() },
    )?;
    let crate::types::Choice {
        message: mut msg,
        finish_reason,
    } = choice;

    crate::types::merge_reasoning_details_into_reasoning_content(&mut msg);

    if let Some(tx) = out {
        non_stream_emit_sse_for_assistant(&msg, tx, plain_terminal_stream, cancel).await;
    }
    if render_to_terminal {
        render_non_stream_assistant_terminal(&msg, plain_terminal_stream, out.is_none())?;
    }
    if let Some(tx) = out {
        non_stream_emit_parsing_tool_calls_if_needed(&msg, tx, cancel).await;
    }
    debug!(
        target: "crabmate",
        "chat completions 非流式响应 finish_reason={} content_len={} tool_calls={} assistant_preview={}",
        finish_reason,
        message_content_byte_len_for_estimate(&msg.content),
        msg.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        redact::assistant_message_preview_for_log(&msg)
    );
    let terminal_end_reason = stream_end_reason_from_finish_and_message(&finish_reason, &msg);
    append_stream_diagnostic_event(terminal_end_reason.as_str(), &msg);
    if render_to_terminal && out.is_none() {
        let _ = crate::runtime::terminal_cli_transcript::print_stream_end_reason_terminal(
            terminal_end_reason,
        );
    }
    Ok((msg, finish_reason))
}

async fn streaming_chat_response(
    res: reqwest::Response,
    out: Option<&tokio::sync::mpsc::Sender<String>>,
    render_to_terminal: bool,
    cancel: Option<&AtomicBool>,
    cli_terminal_plain: bool,
    thinking_trace_enabled: bool,
    tui_llm_stream_scratch: Option<crate::runtime::tui::TuiLlmStreamScratchArc>,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let _cli_wait_spinner =
        crate::runtime::cli_wait_spinner::CliWaitSpinnerGuard::try_start_for_cli_plain_stream(
            cli_terminal_plain,
        );
    let stream = res.bytes_stream();
    let acc = consume_openai_sse_byte_stream(
        stream,
        cancel,
        out,
        cli_terminal_plain,
        thinking_trace_enabled,
        tui_llm_stream_scratch,
    )
    .await?;

    if render_to_terminal {
        let md = crate::runtime::message_display::assistant_raw_markdown_body_from_parts(
            acc.reasoning_acc.as_str(),
            acc.content_acc.as_str(),
        );
        if cli_terminal_plain {
            finalize_stream_plain_terminal_suffix(
                acc.cli_plain_reasoning_style_active,
                acc.cli_plain_prefix_emitted,
                acc.content_acc.as_str(),
                acc.reasoning_acc.as_str(),
            )?;
        } else if !md.is_empty() {
            terminal_render_agent_markdown(&md)?;
        }
    }

    let finish = if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
        USER_CANCELLED_FINISH_REASON.to_string()
    } else {
        acc.finish_reason.clone()
    };
    let msg = message_from_sse_accum(acc);
    debug!(
        target: "crabmate",
        "chat completions 流式响应拼装完成 finish_reason={} content_len={} tool_calls={} assistant_preview={}",
        finish,
        message_content_byte_len_for_estimate(&msg.content),
        msg.tool_calls.as_ref().map(|t| t.len()).unwrap_or(0),
        redact::assistant_message_preview_for_log(&msg)
    );
    let terminal_end_reason = stream_end_reason_from_finish_and_message(&finish, &msg);
    append_stream_diagnostic_event(terminal_end_reason.as_str(), &msg);
    if render_to_terminal && out.is_none() {
        let _ = crate::runtime::terminal_cli_transcript::print_stream_end_reason_terminal(
            terminal_end_reason,
        );
    }
    Ok((msg, finish))
}

/// 请求 chat/completions：`no_stream == false` 时为 SSE 流式；`true` 时为单次 JSON（`stream: false`）。
/// `plain_terminal_stream` 为 `true` 且 `render_to_terminal && out.is_none()`：流式将 reasoning/content **逐 delta 纯文本**写 stdout（**`reasoning_content`** 与 **`content`** 分色；**`NO_COLOR`/非 TTY** 关闭着色），末尾不再 `markdown_to_ansi`；`--no-stream` 时整段按字段分色一次写出。否则若 `render_to_terminal` 且仍有正文、且未走上述路径，则在整段到达后走 [`terminal_render_agent_markdown`]。
/// 若提供 `out`，流式为每个 content delta；非流式则在有正文时整段发送一次（供 SSE 等）。
///
/// **非流式响应**：按 OpenAI 兼容形 `ChatResponse`（`choices[0].message` + `finish_reason`）反序列化；
/// DeepSeek 等兼容实现可用；字段形态不同的网关需在调用侧适配或扩展解析。
///
/// **DSML 物化**：正文中的 DeepSeek DSML 工具调用**不在**此处解析；由 [`crate::llm::complete_chat_retrying`] 在成功后按配置 **`materialize_deepseek_dsml_tool_calls`** 统一处理。
pub async fn stream_chat(
    params: &super::chat_params::StreamChatParams<'_>,
    req: &mut ChatRequest,
) -> Result<(Message, String), Box<dyn std::error::Error + Send + Sync>> {
    let tui_llm_stream_scratch = params.tui_llm_stream_scratch.clone();
    let super::chat_params::StreamChatParams {
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
        thinking_trace_enabled,
        ..
    } = *params;

    let url = format!(
        "{}/{}",
        api_base.trim_end_matches('/'),
        crate::types::OPENAI_CHAT_COMPLETIONS_REL_PATH
    );
    info!(
        target: "crabmate",
        "发起 chat 请求 url={} model={} streaming={}",
        url,
        req.model,
        !no_stream
    );

    let taken = std::mem::take(&mut req.messages);
    req.messages = crate::agent::message_pipeline::conversation_messages_to_vendor_body(
        &taken,
        fold_system_into_user,
        preserve_reasoning_on_assistant_tool_calls,
        preserve_deepseek_thinking_reasoning_roundtrip,
    );
    log_chat_request_json_preview_if_enabled(req);
    req.stream = Some(!no_stream);

    let mut rb = client.post(&url).json(&req);
    if auth_mode == LlmHttpAuthMode::Bearer {
        rb = rb.header("Authorization", format!("Bearer {}", api_key));
    }
    let res = rb.send().await.map_err(LlmCallError::boxed_from_reqwest)?;
    let res = ensure_chat_completions_success(res)
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { e.into() })?;

    let cli_terminal_plain = render_to_terminal && out.is_none() && plain_terminal_stream;

    if no_stream {
        non_stream_chat_response(
            res,
            out,
            render_to_terminal,
            plain_terminal_stream,
            cancel,
            cli_terminal_plain,
        )
        .await
    } else {
        streaming_chat_response(
            res,
            out,
            render_to_terminal,
            cancel,
            cli_terminal_plain,
            thinking_trace_enabled,
            tui_llm_stream_scratch,
        )
        .await
    }
}
