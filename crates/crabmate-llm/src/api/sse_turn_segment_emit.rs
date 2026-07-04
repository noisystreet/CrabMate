//! LLM 流式解析路径：`tool_call.id` 出现时下发 `turn_segment_start/end`（Turn 布局 Phase 2）。

use std::collections::HashSet;
use std::sync::atomic::AtomicBool;

use crabmate_types::StreamDelta;
use tokio::sync::mpsc::Sender;

use crate::stream_host::StreamChatHost;

use super::sse_parser::{flush_sse_delta_buffer, sse_out_send};

fn merge_tool_call_deltas(
    tcs: Vec<crabmate_types::StreamToolCallDelta>,
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

pub(super) async fn emit_turn_segment_end_if_open(
    host: &dyn StreamChatHost,
    out: Option<&Sender<String>>,
    coop_cancel: Option<&AtomicBool>,
    turn_segment_open: &mut Option<String>,
) -> std::io::Result<()> {
    let Some(segment_id) = turn_segment_open.take() else {
        return Ok(());
    };
    let Some(tx) = out else {
        return Ok(());
    };
    let _ = sse_out_send(
        host,
        tx,
        host.encode_turn_segment_end_sse(segment_id.as_str()),
        "llm::stream_chat turn_segment_end",
        coop_cancel,
    )
    .await;
    Ok(())
}

async fn emit_turn_segment_start_for_tool_call(
    host: &dyn StreamChatHost,
    out: Option<&Sender<String>>,
    coop_cancel: Option<&AtomicBool>,
    tool_call_id: &str,
    turn_segment_open: &mut Option<String>,
    turn_segment_emitted_ids: &mut HashSet<String>,
) -> std::io::Result<()> {
    if tool_call_id.is_empty() || turn_segment_emitted_ids.contains(tool_call_id) {
        return Ok(());
    }
    let Some(tx) = out else {
        turn_segment_emitted_ids.insert(tool_call_id.to_string());
        return Ok(());
    };
    emit_turn_segment_end_if_open(host, Some(tx), coop_cancel, turn_segment_open).await?;
    let _ = sse_out_send(
        host,
        tx,
        host.encode_turn_segment_start_sse(tool_call_id),
        "llm::stream_chat turn_segment_start",
        coop_cancel,
    )
    .await;
    turn_segment_emitted_ids.insert(tool_call_id.to_string());
    *turn_segment_open = Some(format!("seg-before-{tool_call_id}"));
    Ok(())
}

pub(super) struct IngestSseToolCallsFrame<'a> {
    pub host: &'a dyn StreamChatHost,
    pub delta: StreamDelta,
    pub tool_calls_acc: &'a mut Vec<(String, String, String, String)>,
    pub parsing_tool_calls_notified: &'a mut bool,
    pub turn_segment_open: &'a mut Option<String>,
    pub turn_segment_emitted_ids: &'a mut HashSet<String>,
    pub pending_sse_delta: &'a mut String,
    pub out: Option<&'a Sender<String>>,
    pub coop_cancel: Option<&'a AtomicBool>,
}

pub(super) async fn ingest_sse_tool_calls_from_delta(
    frame: IngestSseToolCallsFrame<'_>,
) -> std::io::Result<()> {
    let IngestSseToolCallsFrame {
        host,
        delta,
        tool_calls_acc,
        parsing_tool_calls_notified,
        turn_segment_open,
        turn_segment_emitted_ids,
        pending_sse_delta,
        out,
        coop_cancel,
    } = frame;
    let Some(tcs) = delta.tool_calls else {
        return Ok(());
    };
    if !*parsing_tool_calls_notified && !tcs.is_empty() {
        *parsing_tool_calls_notified = true;
        if let Some(tx) = out {
            flush_sse_delta_buffer(host, pending_sse_delta, Some(tx), coop_cancel).await;
            let _ = sse_out_send(
                host,
                tx,
                host.encode_parsing_tool_calls_sse(),
                "llm::stream_chat parsing_tool_calls notify",
                coop_cancel,
            )
            .await;
        }
    }
    let known_ids: HashSet<String> = tool_calls_acc
        .iter()
        .filter(|t| !t.0.is_empty())
        .map(|t| t.0.clone())
        .collect();
    merge_tool_call_deltas(tcs, tool_calls_acc);
    for (id, _, _, _) in tool_calls_acc.iter() {
        if id.is_empty() || known_ids.contains(id) {
            continue;
        }
        emit_turn_segment_start_for_tool_call(
            host,
            out,
            coop_cancel,
            id.as_str(),
            turn_segment_open,
            turn_segment_emitted_ids,
        )
        .await?;
    }
    Ok(())
}
