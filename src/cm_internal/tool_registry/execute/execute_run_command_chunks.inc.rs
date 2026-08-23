struct RunCommandHostInvoke<'a> {
    env: &'a ToolExecEnv<'a>,
    effective_working_dir: &'a Path,
    workspace_is_set: bool,
    workspace_changed: &'a mut bool,
    web_ctx: Option<&'a WebToolRuntime>,
    name: &'a str,
    args: &'a str,
    tool_call_id: &'a str,
    sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

fn run_command_chunk_sink(
    tool_call_id: String,
    sse_out_tx: Option<tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<crate::cm_sse_protocol::sse::SseControlMirror>,
) -> Option<crate::cm_tools::subprocess_session::SessionChunkSink> {
    if sse_out_tx.is_none() && sse_control_mirror.is_none() {
        return None;
    }
    let seq = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let utf8_out = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let utf8_err = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    Some(std::sync::Arc::new(move |stream, bytes| {
        emit_run_command_tool_output_chunk(
            seq.as_ref(),
            &tool_call_id,
            stream,
            bytes,
            sse_out_tx.as_ref(),
            sse_control_mirror.as_ref(),
            match stream {
                crate::cm_tools::subprocess_session::SessionStream::Stdout => utf8_out.as_ref(),
                crate::cm_tools::subprocess_session::SessionStream::Stderr => utf8_err.as_ref(),
            },
        )
    }))
}

fn emit_run_command_tool_output_chunk(
    seq: &std::sync::atomic::AtomicU64,
    tool_call_id: &str,
    stream: crate::cm_tools::subprocess_session::SessionStream,
    bytes: &[u8],
    sse_out_tx: Option<&tokio::sync::mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::cm_sse_protocol::sse::SseControlMirror>,
    utf8_pending: &std::sync::Mutex<Vec<u8>>,
) -> bool {
    let finish = bytes.is_empty();
    let (text, saved) = {
        let Ok(mut pending) = utf8_pending.lock() else {
            return true;
        };
        let saved = pending.clone();
        let text = crate::cm_tools::subprocess_session::take_utf8_text(&mut pending, bytes, finish);
        (text, saved)
    };
    if text.is_empty() {
        return true;
    }
    let n = seq.load(std::sync::atomic::Ordering::SeqCst) + 1;
    let payload = crate::cm_sse_protocol::sse::protocol::SsePayload::ToolOutputChunk {
        tool_output_chunk: crate::cm_sse_protocol::sse::protocol::ToolOutputChunkBody {
            tool_call_id: tool_call_id.to_string(),
            name: Some("run_command".to_string()),
            seq: n,
            chunk: text,
            stream: Some(stream.as_sse_label().to_string()),
        },
    };
    let encoder = crate::cm_sse_protocol::sse::V2Encoder;
    let ok = crate::cm_sse_protocol::sse::send_sse_control_payload_try_send(
        sse_out_tx,
        sse_control_mirror,
        payload,
        "run_command::output_chunk",
        &encoder,
    );
    if ok {
        seq.store(n, std::sync::atomic::Ordering::SeqCst);
        true
    } else if let Ok(mut pending) = utf8_pending.lock() {
        *pending = saved;
        false
    } else {
        false
    }
}
