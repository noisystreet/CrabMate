//! OpenAI 兼容 SSE：`data:` 行扫描、JSON delta 解析、工具调用累积与可选 `mpsc` 增量下发。

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use futures_util::StreamExt;
use tokio::sync::mpsc::Sender;

use crabmate_types::{StreamChoice, StreamChunk, StreamDelta};

use crate::call_error::LlmCallError;
use crate::stream_host::{DsmlStreamFilter, StreamChatHost, TerminalPlainFragmentCtx};
use crate::stream_scratch::TuiLlmStreamScratchArc;

use super::sse_turn_segment_emit::{
    IngestSseToolCallsFrame, emit_turn_segment_end_if_open, ingest_sse_tool_calls_from_delta,
};

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
async fn emit_thinking_trace_if(
    host: &dyn StreamChatHost,
    enabled: bool,
    out: Option<&Sender<String>>,
    chunk: &str,
    answer_phase: bool,
    coop_cancel: Option<&AtomicBool>,
) -> std::io::Result<()> {
    if !enabled {
        return Ok(());
    }
    let Some(tx) = out else {
        return Ok(());
    };
    let line = if answer_phase {
        host.encode_thinking_trace_answer_phase_sse()
    } else {
        host.encode_thinking_trace_reasoning_delta_sse(chunk)
    };
    let _ = sse_out_send(
        host,
        tx,
        line,
        "llm::stream_chat thinking_trace",
        coop_cancel,
    )
    .await;
    Ok(())
}

/// 将单次 `delta.reasoning_content` 转为应追加到 [`IngestSseState::reasoning_acc`] 的片段。
#[inline]
fn reasoning_content_delta_fragment<'a>(acc: &str, s: &'a str) -> &'a str {
    if s.starts_with(acc) && s.len() >= acc.len() {
        &s[acc.len()..]
    } else {
        s
    }
}

#[inline]
pub(super) async fn sse_out_send(
    host: &dyn StreamChatHost,
    tx: &Sender<String>,
    line: String,
    context: &'static str,
    coop_cancel: Option<&AtomicBool>,
) -> bool {
    host.sse_out_send(tx, line, context, coop_cancel).await
}

struct AccumulateReasoningStreamDeltaCtx<'a> {
    host: &'a dyn StreamChatHost,
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
        ctx.host.cli_terminal_write_plain_fragment(
            fragment,
            TerminalPlainFragmentCtx {
                prefix_emitted: ctx.cli_plain_prefix_emitted,
                reasoning_style_active: ctx.cli_plain_reasoning_style_active,
            },
            true,
        )?;
    }
    if let Some(tx) = ctx.out {
        let _ = sse_out_send(
            ctx.host,
            tx,
            fragment.to_string(),
            "llm::stream_chat ingest delta (reasoning)",
            ctx.coop_cancel,
        )
        .await;
        emit_thinking_trace_if(
            ctx.host,
            ctx.thinking_trace_enabled,
            Some(tx),
            fragment,
            false,
            ctx.coop_cancel,
        )
        .await?;
    }
    Ok(())
}

struct MinimaxReasoningDetailsCtx<'a> {
    host: &'a dyn StreamChatHost,
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
        host,
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
                host,
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
    host: &dyn StreamChatHost,
    pending: &mut String,
    tx: Option<&Sender<String>>,
    coop_cancel: Option<&AtomicBool>,
) {
    if let Some(t) = tx
        && !pending.is_empty()
    {
        let line = std::mem::take(pending);
        let _ = sse_out_send(
            host,
            t,
            line,
            "llm::stream_chat flush_sse_delta_buffer",
            coop_cancel,
        )
        .await;
    }
}

pub(super) struct IngestSseState<'a> {
    pub(super) host: &'a dyn StreamChatHost,
    pub(super) out: Option<&'a Sender<String>>,
    pub(super) pending_sse_delta: &'a mut String,
    pub(super) reasoning_acc: &'a mut String,
    pub(super) content_acc: &'a mut String,
    pub(super) finish_reason: &'a mut String,
    pub(super) tool_calls_acc: &'a mut Vec<(String, String, String, String)>,
    pub(super) parsing_tool_calls_notified: &'a mut bool,
    pub(super) turn_segment_open: &'a mut Option<String>,
    pub(super) turn_segment_emitted_ids: &'a mut HashSet<String>,
    pub(super) cli_terminal_plain: bool,
    pub(super) cli_plain_prefix_emitted: &'a mut bool,
    pub(super) cli_plain_reasoning_style_active: &'a mut bool,
    pub(super) minimax_reasoning_snaps: &'a mut Vec<String>,
    pub(super) coop_cancel: Option<&'a AtomicBool>,
    pub(super) thinking_trace_enabled: bool,
    pub(super) tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
    pub(super) dsml_content_filter: &'a mut dyn DsmlStreamFilter,
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
    host: &'a dyn StreamChatHost,
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
    host: &'a dyn StreamChatHost,
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
    dsml_content_filter: &'a mut dyn DsmlStreamFilter,
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
        host,
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
    if let Some(ref s) = delta.reasoning_content
        && !s.is_empty()
        && !has_reasoning_details
    {
        let fragment = reasoning_content_delta_fragment(reasoning_acc.as_str(), s.as_str());
        if !fragment.is_empty() {
            accumulate_reasoning_stream_delta(
                fragment,
                &mut AccumulateReasoningStreamDeltaCtx {
                    host,
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
                host,
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

struct FlushDsmlTailCtx<'a> {
    host: &'a dyn StreamChatHost,
    dsml_content_filter: &'a mut dyn DsmlStreamFilter,
    out: Option<&'a Sender<String>>,
    cli_terminal_plain: bool,
    cli_plain_prefix_emitted: &'a mut bool,
    cli_plain_reasoning_style_active: &'a mut bool,
    coop_cancel: Option<&'a AtomicBool>,
    tui_llm_stream_scratch: Option<&'a TuiLlmStreamScratchArc>,
}

async fn flush_dsml_stream_filter_tail(ctx: FlushDsmlTailCtx<'_>) -> std::io::Result<()> {
    let FlushDsmlTailCtx {
        host,
        dsml_content_filter,
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active,
        coop_cancel,
        tui_llm_stream_scratch,
    } = ctx;
    let display = dsml_content_filter.finish();
    if display.is_empty() {
        return Ok(());
    }
    tui_scratch_push_content(tui_llm_stream_scratch, &display);
    if cli_terminal_plain {
        host.cli_terminal_write_plain_fragment(
            &display,
            TerminalPlainFragmentCtx {
                prefix_emitted: cli_plain_prefix_emitted,
                reasoning_style_active: cli_plain_reasoning_style_active,
            },
            false,
        )?;
    }
    if let Some(tx) = out {
        let _ = sse_out_send(
            host,
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
        host,
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
        flush_sse_delta_buffer(host, pending_sse_delta, Some(tx), coop_cancel).await;
        let _ = sse_out_send(
            host,
            tx,
            host.encode_assistant_answer_phase_sse(),
            "llm::stream_chat assistant_answer_phase",
            coop_cancel,
        )
        .await;
        emit_thinking_trace_if(
            host,
            thinking_trace_enabled,
            Some(tx),
            "",
            true,
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
        host.cli_terminal_write_plain_fragment(
            &display,
            TerminalPlainFragmentCtx {
                prefix_emitted: cli_plain_prefix_emitted,
                reasoning_style_active: cli_plain_reasoning_style_active,
            },
            false,
        )?;
    }
    if let Some(tx) = out {
        let _ = sse_out_send(
            host,
            tx,
            display,
            "llm::stream_chat ingest delta (content)",
            coop_cancel,
        )
        .await;
    }
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
        host,
        out,
        pending_sse_delta,
        reasoning_acc,
        content_acc,
        finish_reason,
        tool_calls_acc,
        parsing_tool_calls_notified,
        turn_segment_open,
        turn_segment_emitted_ids,
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
        host,
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
        host,
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
    ingest_sse_tool_calls_from_delta(IngestSseToolCallsFrame {
        host,
        delta,
        tool_calls_acc,
        parsing_tool_calls_notified,
        turn_segment_open,
        turn_segment_emitted_ids,
        pending_sse_delta,
        out,
        coop_cancel,
    })
    .await?;
    Ok(())
}

pub(super) struct SseStreamAccum {
    pub(super) reasoning_acc: String,
    pub(super) content_acc: String,
    pub(super) tool_calls_acc: Vec<(String, String, String, String)>,
    pub(super) finish_reason: String,
    pub(super) cli_plain_prefix_emitted: bool,
    pub(super) cli_plain_reasoning_style_active: bool,
}

pub(super) struct ConsumeSseStreamOpts<'a> {
    pub cancel: Option<&'a AtomicBool>,
    pub out: Option<&'a Sender<String>>,
    pub cli_terminal_plain: bool,
    pub thinking_trace_enabled: bool,
    pub dsml_stream_strip_enabled: bool,
    pub tui_llm_stream_scratch: Option<TuiLlmStreamScratchArc>,
}

pub(super) async fn consume_openai_sse_byte_stream<S, B>(
    host: &dyn StreamChatHost,
    mut stream: S,
    opts: ConsumeSseStreamOpts<'_>,
) -> Result<SseStreamAccum, Box<dyn std::error::Error + Send + Sync>>
where
    S: futures_util::Stream<Item = Result<B, reqwest::Error>> + Unpin,
    B: AsRef<[u8]>,
{
    let ConsumeSseStreamOpts {
        cancel,
        out,
        cli_terminal_plain,
        thinking_trace_enabled,
        dsml_stream_strip_enabled,
        tui_llm_stream_scratch,
    } = opts;
    let mut buf = Vec::new();
    let mut reasoning_acc = String::new();
    let mut content_acc = String::new();
    let mut pending_sse_delta = String::new();
    let mut tool_calls_acc: Vec<(String, String, String, String)> = Vec::new();
    let mut finish_reason = String::new();
    let mut parsing_tool_calls_notified = false;
    let mut turn_segment_open: Option<String> = None;
    let mut turn_segment_emitted_ids: HashSet<String> = HashSet::new();

    let mut cli_plain_prefix_emitted = false;
    let mut cli_plain_reasoning_style_active = false;
    let mut minimax_reasoning_snaps: Vec<String> = Vec::new();
    let mut dsml_content_filter = host.new_dsml_stream_filter(dsml_stream_strip_enabled);
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
                    host,
                    out,
                    pending_sse_delta: &mut pending_sse_delta,
                    reasoning_acc: &mut reasoning_acc,
                    content_acc: &mut content_acc,
                    finish_reason: &mut finish_reason,
                    tool_calls_acc: &mut tool_calls_acc,
                    parsing_tool_calls_notified: &mut parsing_tool_calls_notified,
                    turn_segment_open: &mut turn_segment_open,
                    turn_segment_emitted_ids: &mut turn_segment_emitted_ids,
                    cli_terminal_plain,
                    cli_plain_prefix_emitted: &mut cli_plain_prefix_emitted,
                    cli_plain_reasoning_style_active: &mut cli_plain_reasoning_style_active,
                    minimax_reasoning_snaps: &mut minimax_reasoning_snaps,
                    coop_cancel: cancel,
                    thinking_trace_enabled,
                    tui_llm_stream_scratch: tui_llm_stream_scratch.clone(),
                    dsml_content_filter: dsml_content_filter.as_mut(),
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
            host,
            out,
            pending_sse_delta: &mut pending_sse_delta,
            reasoning_acc: &mut reasoning_acc,
            content_acc: &mut content_acc,
            finish_reason: &mut finish_reason,
            tool_calls_acc: &mut tool_calls_acc,
            parsing_tool_calls_notified: &mut parsing_tool_calls_notified,
            turn_segment_open: &mut turn_segment_open,
            turn_segment_emitted_ids: &mut turn_segment_emitted_ids,
            cli_terminal_plain,
            cli_plain_prefix_emitted: &mut cli_plain_prefix_emitted,
            cli_plain_reasoning_style_active: &mut cli_plain_reasoning_style_active,
            minimax_reasoning_snaps: &mut minimax_reasoning_snaps,
            coop_cancel: cancel,
            thinking_trace_enabled,
            tui_llm_stream_scratch: tui_llm_stream_scratch.clone(),
            dsml_content_filter: dsml_content_filter.as_mut(),
        },
    )
    .await?;

    emit_turn_segment_end_if_open(host, out, cancel, &mut turn_segment_open).await?;

    flush_sse_delta_buffer(host, &mut pending_sse_delta, out, cancel).await;
    flush_dsml_stream_filter_tail(FlushDsmlTailCtx {
        host,
        dsml_content_filter: dsml_content_filter.as_mut(),
        out,
        cli_terminal_plain,
        cli_plain_prefix_emitted: &mut cli_plain_prefix_emitted,
        cli_plain_reasoning_style_active: &mut cli_plain_reasoning_style_active,
        coop_cancel: cancel,
        tui_llm_stream_scratch: tui_llm_stream_scratch.as_ref(),
    })
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
