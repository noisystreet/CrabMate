//! 控制面 JSON 的 `try_dispatch` 与各 `dispatch_*` / `handle_*` 实现。
//!
//! **分支顺序**与 crate 根 `mod` 文档及 **`crabmate_sse_protocol::classify_sse_control_outcome`** 一致。

use crabmate_sse_protocol::{
    SSE_PROTOCOL_VERSION, classify_sse_control_outcome, extract_clarification_questionnaire,
    extract_error_stop, extract_staged_plan_step_finished, extract_staged_plan_step_started,
    extract_thinking_trace, extract_timeline_log, extract_tool_call, extract_tool_output_chunk,
    extract_tool_result, extract_turn_segment_end, extract_turn_segment_start,
    key_present_non_null,
};
use serde_json::Value;

use super::types::{
    ClarificationFormField, ClarificationQuestionnaireInfo, CommandApprovalRequest, SseControlSink,
    SseDispatch, StagedPlanStepEndInfo, StagedPlanStepStartInfo, ThinkingTraceInfo,
    TimelineLogInfo, ToolOutputChunkInfo, ToolResultInfo, TurnSegmentStartInfo,
};

fn handle_error_stop(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let err = extract_error_stop(obj)?;
    let line = match err.reason_code {
        Some(r) => format!("{} ({}, reason_code={r})", err.message, err.code),
        None => format!("{} ({})", err.message, err.code),
    };
    (sink.on_error)(line);
    Some(SseDispatch::Stop)
}

fn handle_clarification_questionnaire(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let q = extract_clarification_questionnaire(obj)?;
    if let Some(f) = sink.clarify_trace.on_clarification_questionnaire.as_mut() {
        let fields: Vec<ClarificationFormField> = q
            .fields
            .into_iter()
            .map(|x| ClarificationFormField {
                id: x.id,
                label: x.label,
                hint: x.hint,
                required: x.required,
            })
            .collect();
        f(ClarificationQuestionnaireInfo {
            questionnaire_id: q.questionnaire_id,
            intro: q.intro,
            fields,
        });
    }
    Some(SseDispatch::Handled)
}

fn handle_thinking_trace(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let tt = extract_thinking_trace(obj)?;
    if let Some(f) = sink.clarify_trace.on_thinking_trace.as_mut() {
        f(ThinkingTraceInfo {
            op: tt.op,
            node_id: tt.node_id,
            parent_id: tt.parent_id,
            title: tt.title,
            chunk: tt.chunk,
            context_snapshot: tt.context_snapshot,
        });
    }
    Some(SseDispatch::Handled)
}

fn handle_tool_call(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let tc = extract_tool_call(obj)?;
    if let Some(f) = sink.workspace_tool.on_tool_call.as_mut() {
        f(
            tc.name,
            tc.summary,
            tc.arguments_preview,
            tc.arguments,
            tc.goal_id,
            tc.tool_call_id,
        );
    }
    Some(SseDispatch::Handled)
}

fn handle_tool_output_chunk(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let parsed = extract_tool_output_chunk(obj)?;
    let info = ToolOutputChunkInfo {
        tool_call_id: parsed.tool_call_id,
        name: parsed.name,
        seq: parsed.seq,
        chunk: parsed.chunk,
        stream: parsed.stream,
    };
    if let Some(f) = sink.workspace_tool.on_tool_output_chunk.as_mut() {
        f(info);
    }
    Some(SseDispatch::Handled)
}

fn handle_tool_result(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let parsed = extract_tool_result(obj)?;
    let info = ToolResultInfo {
        name: parsed.name,
        goal_id: parsed.goal_id,
        tool_call_id: parsed.tool_call_id,
        result_version: parsed.result_version,
        summary: parsed.summary,
        output: parsed.output,
        ok: parsed.ok,
        exit_code: parsed.exit_code,
        error_code: parsed.error_code,
        failure_category: parsed.failure_category,
        structured_preview: parsed.structured_preview,
    };
    if let Some(f) = sink.workspace_tool.on_tool_result.as_mut() {
        f(info);
    }
    Some(SseDispatch::Handled)
}

fn handle_timeline_log(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let log = extract_timeline_log(obj)?;
    if let Some(f) = sink.notice_timeline.on_timeline_log.as_mut() {
        f(TimelineLogInfo {
            kind: log.kind,
            title: log.title,
            detail: log.detail,
        });
    }
    Some(SseDispatch::Handled)
}

fn handle_sse_capabilities(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    if !key_present_non_null(obj, "sse_capabilities") {
        return None;
    }
    if let Some(Value::Object(caps)) = obj.get("sse_capabilities")
        && let Some(sv_raw) = caps.get("supported_sse_v")
    {
        let sv = sv_raw
            .as_u64()
            .and_then(|n| u8::try_from(n).ok())
            .or_else(|| sv_raw.as_i64().and_then(|n| u8::try_from(n).ok()));
        if let Some(sv) = sv
            && sv != SSE_PROTOCOL_VERSION
        {
            let hint = if sv > SSE_PROTOCOL_VERSION {
                "SSE_SERVER_TOO_NEW"
            } else {
                "SSE_SERVER_TOO_OLD"
            };
            (sink.on_error)(crate::i18n::sse_protocol_version_mismatch(
                sink.user_locale,
                sv,
                SSE_PROTOCOL_VERSION,
                hint,
            ));
            return Some(SseDispatch::Stop);
        }
    }
    Some(SseDispatch::Handled)
}

/// 解析 `data:` 行内容（已去掉 `data: ` 前缀）；非 JSON 或解析失败时返回 `Plain`。
pub fn try_dispatch_sse_control_payload(data: &str, sink: &mut SseControlSink<'_>) -> SseDispatch {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return SseDispatch::Plain;
    };
    let Some(obj) = v.as_object() else {
        return SseDispatch::Plain;
    };
    if classify_sse_control_outcome(&v) == "plain" {
        return SseDispatch::Plain;
    }

    if let Some(d) = handle_error_stop(obj, sink) {
        return d;
    }

    if let Some(d) = dispatch_staged_plan_control(obj, sink) {
        return d;
    }

    if let Some(d) = handle_clarification_questionnaire(obj, sink) {
        return d;
    }

    if let Some(d) = handle_thinking_trace(obj, sink) {
        return d;
    }

    if let Some(d) = dispatch_workspace_tool_control(obj, sink) {
        return d;
    }

    if let Some(d) = dispatch_notice_timeline_tail(obj, sink) {
        return d;
    }

    if classify_sse_control_outcome(&v) == "handled" {
        return SseDispatch::Handled;
    }

    SseDispatch::Plain
}

fn dispatch_turn_layout_control(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    if let Some(seg) = extract_turn_segment_start(obj)
        && let Some(f) = sink.staged_plan.on_turn_segment_start.as_mut()
    {
        f(TurnSegmentStartInfo {
            segment_id: seg.segment_id,
            kind: seg.kind,
            before_tool_call_id: seg.before_tool_call_id,
        });
        return Some(SseDispatch::Handled);
    }
    if let Some(segment_id) = extract_turn_segment_end(obj)
        && let Some(f) = sink.staged_plan.on_turn_segment_end.as_mut()
    {
        f(segment_id);
        return Some(SseDispatch::Handled);
    }
    if obj.get("turn_tool_phase_end") == Some(&Value::Bool(true))
        && let Some(f) = sink.staged_plan.on_turn_tool_phase_end.as_mut()
    {
        f();
        return Some(SseDispatch::Handled);
    }
    None
}

fn dispatch_staged_plan_step_control(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    if key_present_non_null(obj, "staged_plan_step_started") {
        if let Some(info) = extract_staged_plan_step_started(obj)
            && let Some(f) = sink.staged_plan.on_staged_plan_step_started.as_mut()
        {
            f(StagedPlanStepStartInfo {
                step_index: info.step_index,
                total_steps: info.total_steps,
                description: info.description,
                executor_kind: info.executor_kind,
            });
        }
        return Some(SseDispatch::Handled);
    }
    if key_present_non_null(obj, "staged_plan_step_finished") {
        if let Some(info) = extract_staged_plan_step_finished(obj)
            && let Some(f) = sink.staged_plan.on_staged_plan_step_finished.as_mut()
        {
            f(StagedPlanStepEndInfo {
                step_index: info.step_index,
                total_steps: info.total_steps,
                status: info.status,
                executor_kind: info.executor_kind,
            });
        }
        return Some(SseDispatch::Handled);
    }
    None
}

fn dispatch_staged_plan_control(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    if obj.get("plan_required") == Some(&Value::Bool(true)) {
        return Some(SseDispatch::Handled);
    }

    if let Some(Value::Bool(b)) = obj.get("assistant_answer_phase") {
        if *b && let Some(f) = sink.staged_plan.on_assistant_answer_phase.as_mut() {
            f();
        }
        return Some(SseDispatch::Handled);
    }

    if let Some(d) = dispatch_turn_layout_control(obj, sink) {
        return Some(d);
    }

    if key_present_non_null(obj, "staged_plan_started") {
        return Some(SseDispatch::Handled);
    }
    if let Some(d) = dispatch_staged_plan_step_control(obj, sink) {
        return Some(d);
    }
    if key_present_non_null(obj, "staged_plan_finished") {
        return Some(SseDispatch::Handled);
    }
    None
}

fn dispatch_workspace_tool_control(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    if obj.get("workspace_changed") == Some(&Value::Bool(true)) {
        if let Some(f) = sink.workspace_tool.on_workspace_changed.as_mut() {
            f();
        }
        return Some(SseDispatch::Handled);
    }

    if let Some(d) = handle_tool_call(obj, sink) {
        return Some(d);
    }

    if let Some(Value::Bool(b)) = obj.get("parsing_tool_calls") {
        if let Some(f) = sink.workspace_tool.on_parsing_tool_calls_change.as_mut() {
            f(*b);
        }
        return Some(SseDispatch::Handled);
    }
    if let Some(Value::Bool(b)) = obj.get("tool_running") {
        if let Some(f) = sink.workspace_tool.on_tool_status_change.as_mut() {
            f(*b);
        }
        return Some(SseDispatch::Handled);
    }

    if let Some(d) = handle_tool_output_chunk(obj, sink) {
        return Some(d);
    }

    if let Some(d) = handle_tool_result(obj, sink) {
        return Some(d);
    }

    if key_present_non_null(obj, "command_approval_request") {
        if let Some(Value::Object(ar)) = obj.get("command_approval_request") {
            let req = CommandApprovalRequest {
                command: ar
                    .get("command")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                args: ar
                    .get("args")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string(),
                allowlist_key: ar
                    .get("allowlist_key")
                    .and_then(|x| x.as_str())
                    .map(String::from),
            };
            if let Some(f) = sink.workspace_tool.on_command_approval_request.as_mut() {
                f(req);
            }
        }
        return Some(SseDispatch::Handled);
    }
    None
}

fn dispatch_notice_timeline_tail(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    if obj.get("staged_plan_notice").is_some_and(|x| x.is_string())
        || obj.get("staged_plan_notice_clear") == Some(&Value::Bool(true))
    {
        return Some(SseDispatch::Handled);
    }

    if let Some(Value::Bool(_)) = obj.get("chat_ui_separator") {
        return Some(SseDispatch::Handled);
    }
    if key_present_non_null(obj, "conversation_saved") {
        if let Some(Value::Object(saved)) = obj.get("conversation_saved")
            && let Some(rev) = saved.get("revision").and_then(|x| x.as_u64())
            && let Some(f) = sink.notice_timeline.on_conversation_saved_revision.as_mut()
        {
            let tiktoken = saved.get("tiktoken_prompt_tokens").and_then(
                crate::conversation_prompt_tokens_apply::parse_tiktoken_prompt_tokens_value,
            );
            f(rev, tiktoken);
        }
        return Some(SseDispatch::Handled);
    }

    if let Some(d) = handle_timeline_log(obj, sink) {
        return Some(d);
    }

    if let Some(d) = handle_sse_capabilities(obj, sink) {
        return Some(d);
    }
    if key_present_non_null(obj, "stream_ended") {
        return Some(SseDispatch::Handled);
    }
    None
}
