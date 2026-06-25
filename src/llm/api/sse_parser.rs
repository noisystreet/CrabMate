//! OpenAI 兼容 SSE：`data:` 行扫描、JSON delta 解析、工具调用累积与可选 `mpsc` 增量下发。

use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use tokio::sync::mpsc::Sender;

use crate::sse::{SsePayload, ThinkingTraceBody, encode_message};
use crabmate_llm::stream_scratch::TuiLlmStreamScratchArc;
use crabmate_types::{StreamChoice, StreamChunk, StreamDelta};

use super::terminal_render::cli_terminal_write_plain_fragment;
use crabmate_llm::call_error::LlmCallError;

const THINKING_TRACE_CHUNK_MAX: usize = 4096;

#[inline]
fn tui_scratch_push_reasoning(scratch: Option<&TuiLlmStreamScratchArc>, fragment: &str) {
    if fragment.is_empty() {
        return;
    }
    if let Some(a) = scratch
        && let Ok(mut g) = a.lock()
    {
        g.reasoning.push_str(fragment);
    }
}

#[inline]
fn tui_scratch_push_content(scratch: Option<&TuiLlmStreamScratchArc>, fragment: &str) {
    if fragment.is_empty() {
        return;
    }
    if let Some(a) = scratch
        && let Ok(mut g) = a.lock()
    {
        g.content.push_str(fragment);
    }
}

#[inline]
fn clip_thinking_trace_text(s: &str, max: usize) -> String {
    let mut t = s.to_string();
    if t.len() > max {
        t.truncate(max);
        t.push('…');
    }
    t
}

#[inline]
async fn emit_thinking_trace_if(
    enabled: bool,
    out: Option<&Sender<String>>,
    body: ThinkingTraceBody,
    coop_cancel: Option<&AtomicBool>,
) -> std::io::Result<()> {
    if !enabled {
        return Ok(());
    }
    let Some(tx) = out else {
        return Ok(());
    };
    let _ = sse_out_send(
        tx,
        encode_message(SsePayload::ThinkingTrace { trace: body }),
        "llm::stream_chat thinking_trace",
        coop_cancel,
    )
    .await;
    Ok(())
}

/// 将单次 `delta.reasoning_content` 转为应追加到 [`IngestSseState::reasoning_acc`] 的片段。
///
/// - **真增量**（如常见 `reasoning_content` 流）：`s` 通常不是已累积 `acc` 的前缀延长，返回整段 `s`。
/// - **累积快照**（部分网关每帧重发当前全文）：`s` 以 `acc` 为前缀时只返回新增后缀，避免整段重复拼接。
#[inline]
fn reasoning_content_delta_fragment<'a>(acc: &str, s: &'a str) -> &'a str {
    if s.starts_with(acc) && s.len() >= acc.len() {
        &s[acc.len()..]
    } else {
        s
    }
}

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

struct AccumulateReasoningStreamDeltaCtx<'a> {
    reasoning_acc: &'a mut String,
    out: Option<&'a Sender<String>>,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &'a mut bool,
    cli_plain_reasoning_style_active: &'a mut bool,
    coop_cancel: Option<&'a AtomicBool>,
    thinking_trace_enabled: bool,
    tui_llm_stream_scratch: Option<&'a TuiLlmStreamScratchArc>,
}

async fn accumulate_reasoning_stream_delta(
    fragment: &str,
    ctx: &mut AccumulateReasoningStreamDeltaCtx<'_>,
) -> std::io::Result<()> {
    if fragment.is_empty() {
        return Ok(());
    }
    ctx.reasoning_acc.push_str(fragment);
    tui_scratch_push_reasoning(ctx.tui_llm_stream_scratch, fragment);
    if ctx.cli_terminal_plain {
        cli_terminal_write_plain_fragment(
            fragment,
            ctx.cli_plain_prefix_emitted,
            true,
            ctx.cli_plain_reasoning_style_active,
        )?;
    }
    if let Some(tx) = ctx.out {
        // Web/SSE：立即下发，保证聊天区逐 token/逐段更新；CLI `out.is_none()` 时不走此分支。
        let _ = sse_out_send(
            tx,
            fragment.to_string(),
            "llm::stream_chat ingest delta (reasoning)",
            ctx.coop_cancel,
        )
        .await;
        emit_thinking_trace_if(
            ctx.thinking_trace_enabled,
            Some(tx),
            ThinkingTraceBody {
                op: "reasoning_delta".into(),
                node_id: Some("stream_reasoning".into()),
                parent_id: None,
                title: None,
                chunk: Some(clip_thinking_trace_text(fragment, THINKING_TRACE_CHUNK_MAX)),
                context_snapshot: None,
            },
            ctx.coop_cancel,
        )
        .await?;
    }
    Ok(())
}

/// MiniMax `reasoning_split` 流式：`reasoning_details[].text` 多为相对上一块的**累积**全文。
struct MinimaxReasoningDetailsCtx<'a> {
    snaps: &'a mut Vec<String>,
    reasoning_acc: &'a mut String,
    out: Option<&'a Sender<String>>,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &'a mut bool,
    cli_plain_reasoning_style_active: &'a mut bool,
    coop_cancel: Option<&'a AtomicBool>,
    thinking_trace_enabled: bool,
    tui_llm_stream_scratch: Option<&'a TuiLlmStreamScratchArc>,
}

async fn accumulate_minimax_reasoning_details_deltas(
    details: &[serde_json::Value],
    ctx: MinimaxReasoningDetailsCtx<'_>,
) -> std::io::Result<()> {
    let MinimaxReasoningDetailsCtx {
        snaps,
        reasoning_acc,
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
        thinking_trace_enabled,
        tui_llm_stream_scratch,
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
            &mut AccumulateReasoningStreamDeltaCtx {
                reasoning_acc,
                out,
                cli_terminal_plain,
                cli_plain_prefix_emitted,
                cli_plain_reasoning_style_active,
                coop_cancel,
                thinking_trace_enabled,
                tui_llm_stream_scratch,
            },
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
    pub(super) thinking_trace_enabled: bool,
    pub(super) tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
    pub(super) dsml_content_filter: &'a mut crate::dsml::StreamingDsmlContentFilter,
}

async fn ingest_sse_residual_buffer_if_needed(
    stream_done: bool,
    buf: &[u8],
    state: IngestSseState<'_>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if stream_done || buf.is_empty() {
        return Ok(());
    }
    let line = String::from_utf8_lossy(buf);
    let line = line.trim();
    if !line.starts_with("data: ") {
        return Ok(());
    }
    let payload = line.strip_prefix("data: ").unwrap_or("").trim();
    if payload == "[DONE]" {
        return Ok(());
    }
    ingest_sse_data_payload(payload, state)
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
    Ok(())
}

/// 处理已 trim 的一行 SSE；返回 `true` 表示该行宣告 `[DONE]`。
async fn ingest_openai_sse_trimmed_line(
    line: &str,
    state: IngestSseState<'_>,
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let Some(payload) = line.strip_prefix("data: ").map(str::trim) else {
        return Ok(false);
    };
    if payload == "[DONE]" {
        return Ok(true);
    }
    ingest_sse_data_payload(payload, state)
        .await
        .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?;
    Ok(false)
}

struct IngestSseReasoningFrame<'a> {
    delta: &'a StreamDelta,
    reasoning_acc: &'a mut String,
    minimax_reasoning_snaps: &'a mut Vec<String>,
    out: Option<&'a Sender<String>>,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &'a mut bool,
    cli_plain_reasoning_style_active: &'a mut bool,
    coop_cancel: Option<&'a AtomicBool>,
    thinking_trace_enabled: bool,
    tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
}

struct IngestSseContentFrame<'a> {
    delta: &'a StreamDelta,
    content_acc: &'a mut String,
    pending_sse_delta: &'a mut String,
    out: Option<&'a Sender<String>>,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &'a mut bool,
    cli_plain_reasoning_style_active: &'a mut bool,
    coop_cancel: Option<&'a AtomicBool>,
    thinking_trace_enabled: bool,
    tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
    dsml_content_filter: &'a mut crate::dsml::StreamingDsmlContentFilter,
}

#[inline]
fn ingest_sse_apply_finish_reason(finish_reason: &mut String, choice: &StreamChoice) {
    if let Some(reason) = choice.finish_reason.as_ref().filter(|r| !r.is_empty()) {
        *finish_reason = reason.clone();
    }
}

async fn ingest_sse_reasoning_from_delta(
    frame: IngestSseReasoningFrame<'_>,
) -> std::io::Result<()> {
    let IngestSseReasoningFrame {
        delta,
        reasoning_acc,
        minimax_reasoning_snaps,
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
        thinking_trace_enabled,
        tui_llm_stream_scratch,
    } = frame;
    let has_reasoning_details = delta
        .reasoning_details
        .as_ref()
        .is_some_and(|d| !d.is_empty());
    // 同一帧若同时带 `reasoning_details`（如 MiniMax `reasoning_split`）与 `reasoning_content`，
    // 二者往往同源；只走 `reasoning_details` 路径，避免思维链在 UI/会话里重复一份。
    if let Some(ref s) = delta.reasoning_content
        && !s.is_empty()
        && !has_reasoning_details
    {
        let fragment = reasoning_content_delta_fragment(reasoning_acc.as_str(), s.as_str());
        if !fragment.is_empty() {
            accumulate_reasoning_stream_delta(
                fragment,
                &mut AccumulateReasoningStreamDeltaCtx {
                    reasoning_acc,
                    out,
                    cli_terminal_plain,
                    cli_plain_prefix_emitted,
                    cli_plain_reasoning_style_active,
                    coop_cancel,
                    thinking_trace_enabled,
                    tui_llm_stream_scratch: tui_llm_stream_scratch.as_ref(),
                },
            )
            .await?;
        }
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
                cli_terminal_plain,
                cli_plain_prefix_emitted,
                cli_plain_reasoning_style_active,
                coop_cancel,
                thinking_trace_enabled,
                tui_llm_stream_scratch: tui_llm_stream_scratch.as_ref(),
            },
        )
        .await?;
    }
    Ok(())
}

async fn flush_dsml_stream_filter_tail(
    dsml_content_filter: &mut crate::dsml::StreamingDsmlContentFilter,
    out: Option<&Sender<String>>,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &mut bool,
    cli_plain_reasoning_style_active: &mut bool,
    coop_cancel: Option<&AtomicBool>,
    tui_llm_stream_scratch: Option<&TuiLlmStreamScratchArc>,
) -> std::io::Result<()> {
    let display = dsml_content_filter.finish();
    if display.is_empty() {
        return Ok(());
    }
    tui_scratch_push_content(tui_llm_stream_scratch, &display);
    if cli_terminal_plain {
        cli_terminal_write_plain_fragment(
            &display,
            cli_plain_prefix_emitted,
            false,
            cli_plain_reasoning_style_active,
        )?;
    }
    if let Some(tx) = out {
        let _ = sse_out_send(
            tx,
            display,
            "llm::stream_chat flush dsml filter tail",
            coop_cancel,
        )
        .await;
    }
    Ok(())
}

async fn ingest_sse_content_from_delta(frame: IngestSseContentFrame<'_>) -> std::io::Result<()> {
    let IngestSseContentFrame {
        delta,
        content_acc,
        pending_sse_delta,
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
        thinking_trace_enabled,
        tui_llm_stream_scratch,
        dsml_content_filter,
    } = frame;
    let Some(s) = delta.content.as_ref() else {
        return Ok(());
    };
    if s.is_empty() {
        return Ok(());
    }
    if content_acc.is_empty()
        && let Some(tx) = out
        && !cli_terminal_plain
    {
        flush_sse_delta_buffer(pending_sse_delta, Some(tx), coop_cancel).await;
        let _ = sse_out_send(
            tx,
            crate::sse::encode_message(crate::sse::SsePayload::AssistantAnswerPhase {
                assistant_answer_phase: true,
            }),
            "llm::stream_chat assistant_answer_phase",
            coop_cancel,
        )
        .await;
        emit_thinking_trace_if(
            thinking_trace_enabled,
            Some(tx),
            ThinkingTraceBody {
                op: "answer_phase".into(),
                node_id: Some("stream_answer".into()),
                parent_id: Some("stream_reasoning".into()),
                title: Some("assistant_answer_phase".into()),
                chunk: None,
                context_snapshot: None,
            },
            coop_cancel,
        )
        .await?;
    }
    content_acc.push_str(s);
    let display = dsml_content_filter.push_chunk(s);
    if display.is_empty() {
        return Ok(());
    }
    tui_scratch_push_content(tui_llm_stream_scratch.as_ref(), &display);
    if cli_terminal_plain {
        cli_terminal_write_plain_fragment(
            &display,
            cli_plain_prefix_emitted,
            false,
            cli_plain_reasoning_style_active,
        )?;
    }
    if let Some(tx) = out {
        let _ = sse_out_send(
            tx,
            display,
            "llm::stream_chat ingest delta (content)",
            coop_cancel,
        )
        .await;
    }
    Ok(())
}

fn ingest_sse_merge_tool_call_deltas(
    tcs: Vec<crate::types::StreamToolCallDelta>,
    tool_calls_acc: &mut Vec<(String, String, String, String)>,
) {
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

async fn ingest_sse_tool_calls_from_delta(
    delta: StreamDelta,
    tool_calls_acc: &mut Vec<(String, String, String, String)>,
    parsing_tool_calls_notified: &mut bool,
    pending_sse_delta: &mut String,
    out: Option<&Sender<String>>,
    coop_cancel: Option<&AtomicBool>,
) -> std::io::Result<()> {
    let Some(tcs) = delta.tool_calls else {
        return Ok(());
    };
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
    ingest_sse_merge_tool_call_deltas(tcs, tool_calls_acc);
    Ok(())
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
        thinking_trace_enabled,
        tui_llm_stream_scratch,
        dsml_content_filter,
    } = state;
    let tui_scratch = tui_llm_stream_scratch.clone();
    let Ok(chunk) = serde_json::from_slice::<StreamChunk>(payload.as_bytes()) else {
        return Ok(());
    };
    let Some(choice) = chunk.choices.and_then(|c| c.into_iter().next()) else {
        return Ok(());
    };
    ingest_sse_apply_finish_reason(finish_reason, &choice);
    let delta = choice.delta;
    ingest_sse_reasoning_from_delta(IngestSseReasoningFrame {
        delta: &delta,
        reasoning_acc,
        minimax_reasoning_snaps,
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
        thinking_trace_enabled,
        tui_llm_stream_scratch: tui_scratch.clone(),
    })
    .await?;
    ingest_sse_content_from_delta(IngestSseContentFrame {
        delta: &delta,
        content_acc,
        pending_sse_delta,
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
        thinking_trace_enabled,
        tui_llm_stream_scratch: tui_scratch,
        dsml_content_filter,
    })
    .await?;
    ingest_sse_tool_calls_from_delta(
        delta,
        tool_calls_acc,
        parsing_tool_calls_notified,
        pending_sse_delta,
        out,
        coop_cancel,
    )
    .await?;
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
    thinking_trace_enabled: bool,
    dsml_stream_strip_enabled: bool,
    tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
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
    let mut dsml_content_filter =
        crate::dsml::StreamingDsmlContentFilter::new(dsml_stream_strip_enabled);
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
            if ingest_openai_sse_trimmed_line(
                line,
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
                    thinking_trace_enabled,
                    tui_llm_stream_scratch: tui_llm_stream_scratch.clone(),
                    dsml_content_filter: &mut dsml_content_filter,
                },
            )
            .await?
            {
                stream_done = true;
                break;
            }
        }
        if consumed > 0 {
            buf.drain(..consumed);
        }
        if cancelled || stream_done {
            break;
        }
    }

    ingest_sse_residual_buffer_if_needed(
        stream_done,
        buf.as_slice(),
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
            thinking_trace_enabled,
            tui_llm_stream_scratch: tui_llm_stream_scratch.clone(),
            dsml_content_filter: &mut dsml_content_filter,
        },
    )
    .await?;

    flush_sse_delta_buffer(&mut pending_sse_delta, out, cancel).await;
    flush_dsml_stream_filter_tail(
        &mut dsml_content_filter,
        out,
        cli_terminal_plain,
        &mut cli_plain_prefix_emitted,
        &mut cli_plain_reasoning_style_active,
        cancel,
        tui_llm_stream_scratch.as_ref(),
    )
    .await?;

    Ok(SseStreamAccum {
        reasoning_acc,
        content_acc,
        tool_calls_acc,
        finish_reason,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
    })
}

#[cfg(test)]
mod reasoning_delta_tests {
    use super::reasoning_content_delta_fragment;

    #[test]
    fn reasoning_content_delta_empty_acc_is_whole_s() {
        assert_eq!(reasoning_content_delta_fragment("", "hello"), "hello");
    }

    #[test]
    fn reasoning_content_delta_incremental_not_prefix_extends_whole_s() {
        assert_eq!(
            reasoning_content_delta_fragment("hello", " world"),
            " world"
        );
    }

    #[test]
    fn reasoning_content_delta_cumulative_yields_suffix_only() {
        assert_eq!(
            reasoning_content_delta_fragment("The user", "The user is asking"),
            " is asking"
        );
    }

    #[test]
    fn reasoning_content_delta_duplicate_snapshot_yields_empty() {
        assert_eq!(reasoning_content_delta_fragment("same", "same"), "");
    }
}
