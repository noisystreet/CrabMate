//! `terminal_session` 输出分片：正文截断与 SSE **`tool_output_chunk`** 下发。

use tokio::sync::mpsc::Sender;

use crate::cm_sse_protocol::sse::{
    SseControlMirror, SseEncoder, SsePayload, ToolOutputChunkBody, send_sse_control_payload_optional,
};

pub(super) fn push_truncated(acc: &mut String, chunk: &str, max_len: usize) {
    let remain = max_len.saturating_sub(acc.len());
    if remain == 0 {
        return;
    }
    if chunk.len() <= remain {
        acc.push_str(chunk);
    } else {
        let mut end = remain;
        while end > 0 && !chunk.is_char_boundary(end) {
            end -= 1;
        }
        acc.push_str(&chunk[..end]);
    }
}

pub(super) async fn emit_tool_chunk(
    seq: &mut u64,
    tool_call_id: &str,
    text: &str,
    out: Option<&Sender<String>>,
    mirror: Option<&SseControlMirror>,
    encoder: &dyn SseEncoder,
) {
    if text.is_empty() {
        return;
    }
    *seq = seq.saturating_add(1);
    let body = ToolOutputChunkBody {
        tool_call_id: tool_call_id.to_string(),
        name: Some("terminal_session".to_string()),
        seq: *seq,
        chunk: text.to_string(),
        stream: Some("combined".to_string()),
    };
    let _ = send_sse_control_payload_optional(
        out,
        mirror,
        SsePayload::ToolOutputChunk {
            tool_output_chunk: body,
        },
        "terminal_session::pty_chunk",
        encoder,
    )
    .await;
}
