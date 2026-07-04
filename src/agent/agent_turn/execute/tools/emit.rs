use log::info;
use tokio::sync::mpsc;

use crate::agent::per_coord::PerCoordinator;
use crate::clarification_questionnaire::clarification_questionnaire_body_if_tool_ok;
use crate::config::AgentConfig;
use crate::sse::{
    SsePayload, ThinkingTraceBody, ToolCallSummary, ToolOutputChunkBody, ToolResultBody,
    TurnSegmentStartBody, encode_message, send_sse_control_payload_optional,
};
use crate::tool_result::{self, NormalizedToolEnvelope, ToolEnvelopeContext, parse_legacy_output};
use crate::tools;
use crate::types::{Message, message_content_byte_len_for_estimate};

use super::EmitToolResultParams;

fn context_snapshot_for_trace(messages: &[Message]) -> String {
    const MAX: usize = 600;
    let n = messages.len();
    let parts: Vec<String> = messages
        .iter()
        .rev()
        .take(6)
        .rev()
        .map(|m| {
            let role = m.role.as_str();
            let mut c = message_content_byte_len_for_estimate(&m.content);
            if let Some(ref r) = m.reasoning_content {
                c = c.saturating_add(r.len());
            }
            format!("{role}:~{c}b")
        })
        .collect();
    let mut s = format!("messages={n} [{}]", parts.join(", "));
    if s.len() > MAX {
        s = crate::tools::output_util::truncate_to_char_boundary(&s, MAX);
        s.push('…');
    }
    s
}

pub(super) async fn emit_thinking_trace_sse(
    out: Option<&mpsc::Sender<String>>,
    cfg: &AgentConfig,
    body: ThinkingTraceBody,
) {
    if !cfg.agent_thinking_trace.agent_thinking_trace_enabled {
        return;
    }
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ThinkingTrace { trace: body }),
        "execute_tools::thinking_trace",
    )
    .await;
}

/// SSE：`SsePayload::ToolResult`（含 stdout/stderr、retryable、信封元数据）。
async fn emit_sse_tool_result(
    out: Option<&mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    name: &str,
    result: &str,
    tool_summary: Option<String>,
    envelope_ctx: Option<ToolEnvelopeContext<'_>>,
) {
    let parsed = parse_legacy_output(name, result);
    let structured_payload = tool_result::structured_payload_for_tool(name, result);
    let summary_for_norm = tool_summary
        .clone()
        .unwrap_or_else(|| format!("tool: {name}"));
    let norm = NormalizedToolEnvelope::from_tool_run(
        name,
        summary_for_norm,
        &parsed,
        result,
        envelope_ctx.as_ref(),
        structured_payload,
    );
    let mut structured_preview = crate::tools::structured_preview::structured_preview_for_tool_sse(
        name,
        result,
        norm.structured_payload.as_ref(),
    );
    if name == "run_command" {
        structured_preview =
            crate::tools::structured_preview::augment_run_command_preview_with_git_diff(
                structured_preview,
                result,
                parsed.stdout.as_str(),
            );
    }
    let stdout = if parsed.stdout.is_empty() {
        None
    } else {
        Some(parsed.stdout)
    };
    let stderr = if parsed.stderr.is_empty() {
        None
    } else {
        Some(parsed.stderr)
    };
    let payload = SsePayload::ToolResult {
        tool_result: ToolResultBody {
            name: norm.name,
            goal_id: None,
            result_version: norm.envelope_version,
            summary: tool_summary,
            output: result.to_string(),
            ok: Some(norm.ok),
            exit_code: norm.exit_code,
            error_code: norm.error_code.clone(),
            failure_category: norm.failure_category.clone(),
            retryable: norm.retryable,
            tool_call_id: norm.tool_call_id,
            execution_mode: norm.execution_mode,
            parallel_batch_id: norm.parallel_batch_id,
            stdout,
            stderr,
            structured_preview,
        },
    };
    let _ = send_sse_control_payload_optional(
        out,
        sse_control_mirror,
        payload,
        "execute_tools::emit_tool_result_sse",
    )
    .await;
}

/// SSE：`SsePayload::ToolRunning`（`out` 为 `None` 时 no-op）。
pub(super) async fn emit_sse_tool_running(
    out: Option<&mpsc::Sender<String>>,
    tool_running: bool,
    log_label: &'static str,
) {
    let Some(tx) = out else {
        return;
    };
    let _ = crate::sse::send_string_logged(
        tx,
        encode_message(SsePayload::ToolRunning { tool_running }),
        log_label,
    )
    .await;
}

/// SSE：`SsePayload::ToolOutputChunk`（长耗时工具 / PTY 输出增量；最终以 `tool_result` 收束）。
///
/// 供将来 **`terminal_session`** 等接入；当前仓库尚无调用方，保留 API 并允许未使用（避免 `-D warnings` 阻塞）。
#[allow(dead_code)]
pub(super) async fn emit_sse_tool_output_chunk(
    out: Option<&mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    body: ToolOutputChunkBody,
) {
    let payload = SsePayload::ToolOutputChunk {
        tool_output_chunk: body,
    };
    let _ = send_sse_control_payload_optional(
        out,
        sse_control_mirror,
        payload,
        "execute_tools::tool_output_chunk",
    )
    .await;
}

pub(super) async fn emit_timeline_log_sse(
    out: Option<&mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    kind: &str,
    title: String,
    detail: Option<String>,
    log_label: &'static str,
) {
    crate::turn_replay_dump::append_turn_replay_event_if_configured(
        kind,
        title.as_str(),
        detail.as_deref(),
    );
    let payload = SsePayload::TimelineLog {
        log: crate::sse::protocol::TimelineLogBody {
            kind: kind.to_string(),
            title,
            detail,
        },
    };
    let _ = send_sse_control_payload_optional(out, sse_control_mirror, payload, log_label).await;
}

pub(super) async fn emit_tool_result_sse_and_append(
    messages: &mut Vec<Message>,
    per_coord: &mut PerCoordinator,
    p: EmitToolResultParams<'_>,
) {
    let tool_t0 = std::time::Instant::now();
    let EmitToolResultParams {
        cfg,
        tool_outcome_recorder,
        out,
        sse_control_mirror,
        clarification_questionnaire_hook,
        echo_terminal_transcript,
        terminal_tool_display_max_chars,
        tool_result_envelope_v1,
        name,
        args,
        id,
        result,
        reflection_inject,
        envelope_ctx,
    } = p;
    let args_parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let tool_summary = if let Some(ref parsed) = args_parsed {
        tools::summarize_tool_call_parsed(name, parsed)
    } else {
        tools::summarize_tool_call(name, args)
    };
    let parsed_for_timeline = parse_legacy_output(name, result.as_str());

    crate::runtime::terminal_cli_transcript::echo_tool_result_transcript(
        echo_terminal_transcript,
        out.is_some(),
        name,
        args,
        tool_summary.as_deref(),
        result.as_str(),
        terminal_tool_display_max_chars,
    );

    emit_sse_tool_result(
        out,
        sse_control_mirror.as_ref(),
        name,
        result.as_str(),
        tool_summary.clone(),
        envelope_ctx,
    )
    .await;

    if let Some(body) = clarification_questionnaire_body_if_tool_ok(name, args, result.as_str()) {
        let payload = SsePayload::ClarificationQuestionnaire {
            clarification_questionnaire: body.clone(),
        };
        let _ = send_sse_control_payload_optional(
            out,
            sse_control_mirror.as_ref(),
            payload,
            "clarification_questionnaire",
        )
        .await;
        if let Some(h) = clarification_questionnaire_hook.as_ref() {
            h(body);
        }
    }

    let status = if parsed_for_timeline.ok {
        "ok"
    } else {
        "failed"
    };
    let detail = tool_summary.as_ref().map(|s| {
        format!(
            "status={status}, summary={s}, exit_code={}",
            parsed_for_timeline
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "-".to_string())
        )
    });
    emit_timeline_log_sse(
        out,
        sse_control_mirror.as_ref(),
        "tool_step_finished",
        name.to_string(),
        detail,
        "execute_tools::timeline tool_step_finished",
    )
    .await;
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "tool_call_finished",
        name,
        Some(&serde_json::json!({
            "tool_call_id": id,
            "tool_name": name,
            "execution_mode": envelope_ctx.map(|e| e.execution_mode),
            "parallel_batch_id": envelope_ctx.and_then(|e| e.parallel_batch_id),
            "ok": parsed_for_timeline.ok,
            "exit_code": parsed_for_timeline.exit_code,
            "error_code": parsed_for_timeline.error_code,
            "failure_category": parsed_for_timeline
                .error_code
                .as_deref()
                .map(|c| crate::tool_result::failure_category_for_error_code(c).as_str().to_string()),
            "retryable": crate::tool_result::tool_error_retryable_heuristic(
                parsed_for_timeline.error_code.as_deref()
            ),
            "summary": tool_summary,
            "stdout_preview": crate::redact::preview_chars(&parsed_for_timeline.stdout, 1200),
            "stdout_preview_truncated": parsed_for_timeline.stdout.chars().count() > 1200,
            "stderr_preview": crate::redact::preview_chars(&parsed_for_timeline.stderr, 1200),
            "stderr_preview_truncated": parsed_for_timeline.stderr.chars().count() > 1200,
            "result_preview": crate::redact::single_line_preview(result.as_str(), 1200),
            "result_preview_truncated": result.chars().count() > 1200,
            "tool_elapsed_ms": tool_t0.elapsed().as_millis(),
            "phase": "tool_execution",
        })),
    );

    tool_outcome_recorder.record_tool_outcome(
        cfg.as_ref(),
        name,
        result.as_str(),
        tool_summary.clone(),
        envelope_ctx.as_ref(),
    );
    tool_outcome_recorder.record_tool_execution_trace(
        name,
        id,
        args,
        parsed_for_timeline.ok,
        tool_t0.elapsed().as_millis() as u64,
    );

    let content_for_model = if tool_result_envelope_v1 {
        let parsed = parse_legacy_output(name, &result);
        let summary_str = tool_summary
            .clone()
            .unwrap_or_else(|| format!("tool: {name}"));
        tool_result::encode_tool_message_envelope_v1(
            name,
            summary_str,
            &parsed,
            &result,
            envelope_ctx.as_ref(),
        )
    } else {
        result
    };

    PerCoordinator::append_tool_result_and_reflection(
        per_coord,
        messages,
        id.to_string(),
        content_for_model,
        reflection_inject,
    );

    emit_thinking_trace_sse(
        out,
        cfg.as_ref(),
        ThinkingTraceBody {
            op: "tool_done".into(),
            node_id: Some(format!("tool:{name}")),
            parent_id: None,
            title: Some(name.to_string()),
            chunk: None,
            context_snapshot: Some(context_snapshot_for_trace(messages)),
        },
    )
    .await;
}

pub(super) async fn emit_turn_segment_start_before_tool_sse(
    out: Option<&mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    tool_call_id: &str,
) {
    if out.is_none() && sse_control_mirror.is_none() {
        return;
    }
    let payload = SsePayload::TurnSegmentStart {
        start: TurnSegmentStartBody {
            segment_id: format!("seg-before-{tool_call_id}"),
            kind: "commentary".to_string(),
            before_tool_call_id: Some(tool_call_id.to_string()),
        },
    };
    let _ = send_sse_control_payload_optional(
        out,
        sse_control_mirror,
        payload,
        "execute_tools::turn_segment_start",
    )
    .await;
}

pub(super) async fn emit_turn_tool_phase_end_sse(
    out: Option<&mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
) {
    if out.is_none() && sse_control_mirror.is_none() {
        return;
    }
    let payload = SsePayload::TurnToolPhaseEnd {
        turn_tool_phase_end: true,
    };
    let _ = send_sse_control_payload_optional(
        out,
        sse_control_mirror,
        payload,
        "execute_tools::turn_tool_phase_end",
    )
    .await;
}

pub(super) async fn emit_tool_call_summary_sse(
    out: Option<&mpsc::Sender<String>>,
    sse_control_mirror: Option<&crate::sse::SseControlMirror>,
    cfg: &AgentConfig,
    tool_call_id: &str,
    name: &str,
    args: &str,
    messages: &[Message],
) {
    let args_preview = crate::redact::tool_arguments_preview_for_sse(args);
    let args_parsed: Option<serde_json::Value> = serde_json::from_str(args).ok();
    let summary = if let Some(ref parsed) = args_parsed {
        tools::summarize_tool_call_parsed(name, parsed)
    } else {
        tools::summarize_tool_call(name, args)
    }
    .unwrap_or_else(|| format!("tool: {name}"));
    let arguments_preview = Some(args_preview.clone());
    let arguments = cfg
        .tool_transcript
        .sse_tool_call_include_arguments
        .then(|| crate::redact::tool_arguments_redacted_for_sse(args));

    let args_for_log = crate::redact::tool_arguments_preview_for_log(args);
    info!(
        target: "crabmate::tool_call",
        "[tool_call] name={} args={}",
        name,
        args_for_log
    );
    crate::turn_replay_dump::append_turn_replay_event_json_if_configured(
        "tool_call_started",
        name,
        Some(&serde_json::json!({
            "tool_call_id": tool_call_id,
            "tool_name": name,
            "summary": summary,
            "args_preview": args_preview,
            "args_preview_truncated": args.chars().count() > 1200,
            "phase": "tool_execution",
        })),
    );

    if out.is_none() && sse_control_mirror.is_none() {
        return;
    }

    emit_turn_segment_start_before_tool_sse(out, sse_control_mirror, tool_call_id).await;

    let payload = SsePayload::ToolCall {
        tool_call: ToolCallSummary {
            name: name.to_string(),
            summary,
            goal_id: None,
            tool_call_id: Some(tool_call_id.to_string()),
            arguments_preview,
            arguments,
        },
    };
    let _ = send_sse_control_payload_optional(
        out,
        sse_control_mirror,
        payload,
        "execute_tools::tool_call summary",
    )
    .await;
    emit_thinking_trace_sse(
        out,
        cfg,
        ThinkingTraceBody {
            op: "tool_call".into(),
            node_id: Some(format!("tool:{name}")),
            parent_id: None,
            title: Some(name.to_string()),
            chunk: None,
            context_snapshot: Some(context_snapshot_for_trace(messages)),
        },
    )
    .await;
}
