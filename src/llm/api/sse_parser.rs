//! OpenAI 兼容 SSE：`data:` 行扫描、JSON delta 解析、工具调用累积与可选 `mpsc` 增量下发。

use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use tokio::sync::mpsc::Sender;

use crate::types::StreamChunk;

use super::super::call_error::LlmCallError;
use super::terminal_render::cli_terminal_write_plain_fragment;

/// 流式正文 delta 在发往 `out`（SSE 等）前合并，减少 `mpsc` 次与 `String` 小片 clone。
pub(super) const SSE_STREAM_DELTA_FLUSH_BYTES: usize = 256;

/// Web 流式：`out` 存在且提供 **`coop_cancel`** 时，发送失败会置位取消标志，与 `chat_job_queue` 的 `closed()` 监视一致。
#[inline]
pub(super) async fn sse_out_send(
    tx: &Sender<String>,
    line: String,
    context: &'static str,
    coop_cancel: Option<&AtomicBool>,
) -> bool {
    match coop_cancel {
        Some(c) => {
            crate::sse::send_string_logged_cooperative_cancel(tx, line, context, Some(c)).await
        }
        None => crate::sse::send_string_logged(tx, line, context).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn accumulate_reasoning_stream_delta(
    fragment: &str,
    reasoning_acc: &mut String,
    out: Option<&Sender<String>>,
    pending_sse_delta: &mut String,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &mut bool,
    cli_plain_reasoning_style_active: &mut bool,
    coop_cancel: Option<&AtomicBool>,
) -> std::io::Result<()> {
    if fragment.is_empty() {
        return Ok(());
    }
    reasoning_acc.push_str(fragment);
    if cli_terminal_plain {
        cli_terminal_write_plain_fragment(
            fragment,
            cli_plain_prefix_emitted,
            true,
            cli_plain_reasoning_style_active,
        )?;
    }
    if let Some(tx) = out {
        pending_sse_delta.push_str(fragment);
        if pending_sse_delta.len() >= SSE_STREAM_DELTA_FLUSH_BYTES {
            let line = std::mem::take(pending_sse_delta);
            let _ = sse_out_send(
                tx,
                line,
                "llm::stream_chat ingest delta (reasoning)",
                coop_cancel,
            )
            .await;
        }
    }
    Ok(())
}

/// MiniMax `reasoning_split` 流式：`reasoning_details[].text` 多为相对上一块的**累积**全文。
struct MinimaxReasoningDetailsCtx<'a> {
    snaps: &'a mut Vec<String>,
    reasoning_acc: &'a mut String,
    out: Option<&'a Sender<String>>,
    pending_sse_delta: &'a mut String,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &'a mut bool,
    cli_plain_reasoning_style_active: &'a mut bool,
    coop_cancel: Option<&'a AtomicBool>,
}

async fn accumulate_minimax_reasoning_details_deltas(
    details: &[serde_json::Value],
    ctx: MinimaxReasoningDetailsCtx<'_>,
) -> std::io::Result<()> {
    let MinimaxReasoningDetailsCtx {
        snaps,
        reasoning_acc,
        out,
        pending_sse_delta,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
    } = ctx;
    while snaps.len() < details.len() {
        snaps.push(String::new());
    }
    for (i, d) in details.iter().enumerate() {
        let Some(obj) = d.as_object() else {
            continue;
        };
        let Some(serde_json::Value::String(t)) = obj.get("text") else {
            continue;
        };
        let snap = &mut snaps[i];
        let fragment = if t.starts_with(snap.as_str()) && t.len() >= snap.len() {
            &t[snap.len()..]
        } else {
            t.as_str()
        };
        accumulate_reasoning_stream_delta(
            fragment,
            reasoning_acc,
            out,
            pending_sse_delta,
            cli_terminal_plain,
            cli_plain_prefix_emitted,
            cli_plain_reasoning_style_active,
            coop_cancel,
        )
        .await?;
        snap.clear();
        snap.push_str(t);
    }
    Ok(())
}

pub(super) async fn flush_sse_delta_buffer(
    pending: &mut String,
    tx: Option<&Sender<String>>,
    coop_cancel: Option<&AtomicBool>,
) {
    if let Some(t) = tx
        && !pending.is_empty()
    {
        let line = std::mem::take(pending);
        let _ = sse_out_send(
            t,
            line,
            "llm::stream_chat flush_sse_delta_buffer",
            coop_cancel,
        )
        .await;
    }
}

/// 解析 SSE 中一行 `data:` 后的 JSON 负载，累积正文与 tool_calls，并经 `out` 下发流式增量。
pub(super) struct IngestSseState<'a> {
    pub(super) out: Option<&'a Sender<String>>,
    pub(super) pending_sse_delta: &'a mut String,
    pub(super) reasoning_acc: &'a mut String,
    pub(super) content_acc: &'a mut String,
    pub(super) finish_reason: &'a mut String,
    pub(super) tool_calls_acc: &'a mut Vec<(String, String, String, String)>,
    pub(super) parsing_tool_calls_notified: &'a mut bool,
    pub(super) cli_terminal_plain: bool,
    pub(super) cli_plain_prefix_emitted: &'a mut bool,
    pub(super) cli_plain_reasoning_style_active: &'a mut bool,
    pub(super) minimax_reasoning_snaps: &'a mut Vec<String>,
    pub(super) coop_cancel: Option<&'a AtomicBool>,
}

pub(super) async fn ingest_sse_data_payload(
    payload: &str,
    state: IngestSseState<'_>,
) -> std::io::Result<()> {
    if payload.is_empty() {
        return Ok(());
    }
    let IngestSseState {
        out,
        pending_sse_delta,
        reasoning_acc,
        content_acc,
        finish_reason,
        tool_calls_acc,
        parsing_tool_calls_notified,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        minimax_reasoning_snaps,
        coop_cancel,
    } = state;
    let Ok(chunk) = serde_json::from_slice::<StreamChunk>(payload.as_bytes()) else {
        return Ok(());
    };
    let Some(choice) = chunk.choices.and_then(|c| c.into_iter().next()) else {
        return Ok(());
    };
    if let Some(reason) = choice.finish_reason
        && !reason.is_empty()
    {
        *finish_reason = reason;
    }
    let delta = choice.delta;
    if let Some(ref s) = delta.reasoning_content
        && !s.is_empty()
    {
        accumulate_reasoning_stream_delta(
            s,
            reasoning_acc,
            out,
            pending_sse_delta,
            cli_terminal_plain,
            cli_plain_prefix_emitted,
            cli_plain_reasoning_style_active,
            coop_cancel,
        )
        .await?;
    }
    if let Some(ref details) = delta.reasoning_details
        && !details.is_empty()
    {
        accumulate_minimax_reasoning_details_deltas(
            details,
            MinimaxReasoningDetailsCtx {
                snaps: minimax_reasoning_snaps,
                reasoning_acc,
                out,
                pending_sse_delta,
                cli_terminal_plain,
                cli_plain_prefix_emitted,
                cli_plain_reasoning_style_active,
                coop_cancel,
            },
        )
        .await?;
    }
    if let Some(ref s) = delta.content
        && !s.is_empty()
    {
        content_acc.push_str(s);
        if cli_terminal_plain {
            cli_terminal_write_plain_fragment(
                s,
                cli_plain_prefix_emitted,
                false,
                cli_plain_reasoning_style_active,
            )?;
        }
        if let Some(tx) = out {
            pending_sse_delta.push_str(s);
            if pending_sse_delta.len() >= SSE_STREAM_DELTA_FLUSH_BYTES {
                let line = std::mem::take(pending_sse_delta);
                let _ = sse_out_send(
                    tx,
                    line,
                    "llm::stream_chat ingest delta (content)",
                    coop_cancel,
                )
                .await;
            }
        }
    }
    if let Some(tcs) = delta.tool_calls {
        if !*parsing_tool_calls_notified && !tcs.is_empty() {
            *parsing_tool_calls_notified = true;
            if let Some(tx) = out {
                flush_sse_delta_buffer(pending_sse_delta, Some(tx), coop_cancel).await;
                let _ = sse_out_send(
                    tx,
                    crate::sse::encode_message(crate::sse::SsePayload::ParsingToolCalls {
                        parsing_tool_calls: true,
                    }),
                    "llm::stream_chat parsing_tool_calls notify",
                    coop_cancel,
                )
                .await;
            }
        }
        for tc in tcs {
            let idx = tc.index;
            while tool_calls_acc.len() <= idx {
                tool_calls_acc.push((
                    String::new(),
                    "function".to_string(),
                    String::new(),
                    String::new(),
                ));
            }
            let acc = &mut tool_calls_acc[idx];
            if let Some(id) = tc.id {
                acc.0 = id;
            }
            if let Some(t) = tc.typ {
                acc.1 = t;
            }
            if let Some(f) = tc.function {
                if let Some(n) = f.name {
                    acc.2 = n;
                }
                if let Some(a) = f.arguments {
                    acc.3.push_str(&a);
                }
            }
        }
    }
    Ok(())
}

/// 流式 SSE 消费结束后的累积状态（供拼装 [`crate::types::Message`]）。
pub(super) struct SseStreamAccum {
    pub(super) reasoning_acc: String,
    pub(super) content_acc: String,
    pub(super) tool_calls_acc: Vec<(String, String, String, String)>,
    pub(super) finish_reason: String,
    pub(super) cli_plain_prefix_emitted: bool,
    pub(super) cli_plain_reasoning_style_active: bool,
}

pub(super) async fn consume_openai_sse_byte_stream<S, B>(
    mut stream: S,
    cancel: Option<&AtomicBool>,
    out: Option<&Sender<String>>,
    cli_terminal_plain: bool,
) -> Result<SseStreamAccum, Box<dyn std::error::Error + Send + Sync>>
where
    S: futures_util::Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    let mut buf = Vec::new();
    let mut reasoning_acc = String::new();
    let mut content_acc = String::new();
    let mut pending_sse_delta = String::new();
    let mut tool_calls_acc: Vec<(String, String, String, String)> = Vec::new();
    let mut finish_reason = String::new();
    let mut parsing_tool_calls_notified = false;

    let mut cli_plain_prefix_emitted = false;
    let mut cli_plain_reasoning_style_active = false;
    let mut minimax_reasoning_snaps: Vec<String> = Vec::new();
    let mut stream_done = false;

    while let Some(chunk) = stream.next().await {
        if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
            break;
        }
        let chunk = chunk.map_err(LlmCallError::boxed_from_reqwest)?;
        buf.extend_from_slice(chunk.as_ref());

        let mut consumed = 0usize;
        let mut cancelled = false;
        while let Some(rel_pos) = buf[consumed..].iter().position(|&b| b == b'\n') {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                cancelled = true;
                break;
            }
            let pos = consumed + rel_pos;
            let line = std::str::from_utf8(&buf[consumed..pos])
                .unwrap_or("")
                .trim();
            consumed = pos + 1;
            if let Some(payload) = line.strip_prefix("data: ").map(str::trim) {
                if payload == "[DONE]" {
                    stream_done = true;
                    break;
                }
                ingest_sse_data_payload(
                    payload,
                    IngestSseState {
                        out,
                        pending_sse_delta: &mut pending_sse_delta,
                        reasoning_acc: &mut reasoning_acc,
                        content_acc: &mut content_acc,
                        finish_reason: &mut finish_reason,
                        tool_calls_acc: &mut tool_calls_acc,
                        parsing_tool_calls_notified: &mut parsing_tool_calls_notified,
                        cli_terminal_plain,
                        cli_plain_prefix_emitted: &mut cli_plain_prefix_emitted,
                        cli_plain_reasoning_style_active: &mut cli_plain_reasoning_style_active,
                        minimax_reasoning_snaps: &mut minimax_reasoning_snaps,
                        coop_cancel: cancel,
                    },
                )
                .await?;
            }
        }
        if consumed > 0 {
            buf.drain(..consumed);
        }
        if cancelled || stream_done {
            break;
        }
    }

    if !stream_done && !buf.is_empty() {
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.starts_with("data: ") {
            let payload = line.strip_prefix("data: ").unwrap_or("").trim();
            if payload != "[DONE]" {
                ingest_sse_data_payload(
                    payload,
                    IngestSseState {
                        out,
                        pending_sse_delta: &mut pending_sse_delta,
                        reasoning_acc: &mut reasoning_acc,
                        content_acc: &mut content_acc,
                        finish_reason: &mut finish_reason,
                        tool_calls_acc: &mut tool_calls_acc,
                        parsing_tool_calls_notified: &mut parsing_tool_calls_notified,
                        cli_terminal_plain,
                        cli_plain_prefix_emitted: &mut cli_plain_prefix_emitted,
                        cli_plain_reasoning_style_active: &mut cli_plain_reasoning_style_active,
                        minimax_reasoning_snaps: &mut minimax_reasoning_snaps,
                        coop_cancel: cancel,
                    },
                )
                .await?;
            }
        }
    }

    flush_sse_delta_buffer(&mut pending_sse_delta, out, cancel).await;

    Ok(SseStreamAccum {
        reasoning_acc,
        content_acc,
        tool_calls_acc,
        finish_reason,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
    })
}
