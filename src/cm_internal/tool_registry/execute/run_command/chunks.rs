//! `run_command` 流水线的 输出分块与宿主调用入参 阶段。
//!
//! 原为 `execute/execute_run_command_chunks.inc.rs`，通过 `include!` 与其余六片共享
//! `execute` 的单一作用域；现为真子模块，跨片调用必须经 `mod.rs` 的显式接口（#1196）。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::cm_config::{AgentConfig, SyncDefaultToolSandboxMode};
use crate::cm_internal::tool_approval::{self, InteractiveGateOutcome};
use crate::cm_internal::tools;
use crate::cm_types::{CommandApprovalDecision, ToolCall};

// 祖先层（`execute`）的项：子模块可见祖先的私有项，故直接 glob 取用。
use super::super::*;
// 同一条流水线的兄弟子模块：只能看到 `mod.rs` 里 `pub(in crate::cm_internal::tool_registry::execute) use` 出来的显式接口。
use super::*;
use crate::cm_names as tool_names;

pub(in crate::cm_internal::tool_registry::execute) struct RunCommandHostInvoke<'a> {
    pub(in crate::cm_internal::tool_registry::execute) env: &'a ToolExecEnv<'a>,
    pub(in crate::cm_internal::tool_registry::execute) effective_working_dir: &'a Path,
    pub(in crate::cm_internal::tool_registry::execute) workspace_is_set: bool,
    pub(in crate::cm_internal::tool_registry::execute) workspace_changed: &'a mut bool,
    pub(in crate::cm_internal::tool_registry::execute) web_ctx: Option<&'a WebToolRuntime>,
    pub(in crate::cm_internal::tool_registry::execute) args: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) tool_call_id: &'a str,
    pub(in crate::cm_internal::tool_registry::execute) sse_out_tx: Option<&'a tokio::sync::mpsc::Sender<String>>,
    pub(in crate::cm_internal::tool_registry::execute) sse_control_mirror: Option<&'a crate::cm_sse_protocol::sse::SseControlMirror>,
    pub(in crate::cm_internal::tool_registry::execute) cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    pub(in crate::cm_internal::tool_registry::execute) tool_jobs: Option<std::sync::Arc<crate::cm_internal::tool_jobs::ToolJobRegistry>>,
}

pub(in crate::cm_internal::tool_registry::execute) fn run_command_chunk_sink(
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

pub(in crate::cm_internal::tool_registry::execute) fn emit_run_command_tool_output_chunk(
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
            name: Some(tool_names::RUN_COMMAND.to_string()),
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
