//! 前端 SSE 控制面 JSON 的分类与分发（`serde_json::Value`）。
//!
//! **`stop`/`handled`/`plain` 分支顺序**须与 workspace crate **`crabmate-sse-protocol`** 中
//! [`classify_sse_control_outcome`](crabmate_sse_protocol::classify_sse_control_outcome) 及
//! **`fixtures/sse_control_golden.jsonl`** 一致（见该 crate 的 `control_classify`）。

use crabmate_sse_protocol::{
    SSE_PROTOCOL_VERSION, classify_sse_control_outcome, extract_clarification_questionnaire,
    extract_error_stop, extract_staged_plan_step_finished, extract_staged_plan_step_started,
    extract_thinking_trace, extract_timeline_log, extract_tool_call, extract_tool_result,
    key_present_non_null,
};
use serde_json::Value;

use crate::i18n::Locale;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseDispatch {
    Stop,
    Handled,
    Plain,
}

/// 工作区与工具相关控制面回调（`tool_call` / `tool_running` / 审批等）。
#[allow(clippy::type_complexity)]
pub struct SseWorkspaceToolHooks<'a> {
    pub on_workspace_changed: Option<&'a mut dyn FnMut()>,
    pub on_tool_call: Option<
        &'a mut dyn FnMut(
            String,
            String,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        ),
    >,
    pub on_tool_status_change: Option<&'a mut dyn FnMut(bool)>,
    pub on_parsing_tool_calls_change: Option<&'a mut dyn FnMut(bool)>,
    pub on_tool_result: Option<&'a mut dyn FnMut(ToolResultInfo)>,
    pub on_command_approval_request: Option<&'a mut dyn FnMut(CommandApprovalRequest)>,
}

/// `assistant_answer_phase` 与分步规划时间线。
pub struct SseStagedPlanHooks<'a> {
    /// 后续 `on_delta` 为终答正文（此前为思维链）；无链时也会在首段正文前下发。
    pub on_assistant_answer_phase: Option<&'a mut dyn FnMut()>,
    pub on_staged_plan_step_started: Option<&'a mut dyn FnMut(StagedPlanStepStartInfo)>,
    pub on_staged_plan_step_finished: Option<&'a mut dyn FnMut(StagedPlanStepEndInfo)>,
}

/// 澄清问卷与思维迹调试事件。
pub struct SseClarifyTraceHooks<'a> {
    pub on_clarification_questionnaire: Option<&'a mut dyn FnMut(ClarificationQuestionnaireInfo)>,
    pub on_thinking_trace: Option<&'a mut dyn FnMut(ThinkingTraceInfo)>,
}

/// 会话落盘 revision、`timeline_log`、协议能力等尾部控制面。
pub struct SseNoticeTimelineHooks<'a> {
    /// `conversation_saved.revision`，供 `POST /chat/branch` 与冲突检测。
    pub on_conversation_saved_revision: Option<&'a mut dyn FnMut(u64)>,
    /// `timeline_log` 事件：审批结果等旁注，写入时间线（不进聊天正文）。
    pub on_timeline_log: Option<&'a mut dyn FnMut(TimelineLogInfo)>,
}

/// SSE 控制面分发入口：按领域分组回调，与 [`try_dispatch_sse_control_payload`] 分支顺序对齐。
pub struct SseControlSink<'a> {
    /// 用户可见错误文案语言（如 SSE 协议版本不匹配提示）。
    pub user_locale: Locale,
    pub on_error: &'a mut dyn FnMut(String),
    pub workspace_tool: SseWorkspaceToolHooks<'a>,
    pub staged_plan: SseStagedPlanHooks<'a>,
    pub clarify_trace: SseClarifyTraceHooks<'a>,
    pub notice_timeline: SseNoticeTimelineHooks<'a>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // 与后端 JSON 同形；展示层当前仅用 name/summary。
pub struct ToolResultInfo {
    pub name: String,
    pub goal_id: Option<String>,
    /// 与对应 `tool_call.tool_call_id` 对齐；缺省时前端按 FIFO 与占位气泡配对。
    pub tool_call_id: Option<String>,
    /// 与 `crabmate_tool.v` 对齐；缺省按 **1**（与后端 `serde(default)` 一致）。
    pub result_version: u32,
    pub summary: Option<String>,
    pub output: String,
    pub ok: Option<bool>,
    pub exit_code: Option<i64>,
    pub error_code: Option<String>,
    /// 与 Rust `tool_error::ToolFailureCategory` 蛇形字符串同源（`invalid_input` 等）。
    pub failure_category: Option<String>,
    /// 可选：与 `read_file` / `read_dir` / `list_tree` 工具输出首行 **`crabmate_tool_output`** 同源（SSE 侧复制），便于 UI 表格化。
    pub structured_preview: Option<Value>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CommandApprovalRequest {
    pub command: String,
    pub args: String,
    pub allowlist_key: Option<String>,
}

/// `staged_plan_step_started`：Web 时间线展示用字段子集。
#[derive(Debug, Clone)]
pub struct StagedPlanStepStartInfo {
    pub step_index: usize,
    pub total_steps: usize,
    pub description: String,
    pub executor_kind: Option<String>,
}

/// `staged_plan_step_finished`：Web 时间线展示用字段子集。
#[derive(Debug, Clone)]
pub struct StagedPlanStepEndInfo {
    pub step_index: usize,
    pub total_steps: usize,
    pub status: String,
    pub executor_kind: Option<String>,
}

/// `clarification_questionnaire`：Web 表单用字段子集。
#[derive(Debug, Clone)]
pub struct ClarificationQuestionnaireInfo {
    pub questionnaire_id: String,
    pub intro: String,
    pub fields: Vec<ClarificationFormField>,
}

#[derive(Debug, Clone)]
pub struct ClarificationFormField {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub required: bool,
}

/// `thinking_trace`：Web 调试台用（不进聊天正文）。
#[derive(Debug, Clone)]
pub struct ThinkingTraceInfo {
    pub op: String,
    pub node_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub chunk: Option<String>,
    pub context_snapshot: Option<String>,
}

/// `timeline_log`：Web 时间线旁注（审批结果等；不进聊天正文）。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TimelineLogInfo {
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
}

fn handle_error_stop(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let Some(err) = extract_error_stop(obj) else {
        return None;
    };
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
    let Some(q) = extract_clarification_questionnaire(obj) else {
        return None;
    };
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
    let Some(tt) = extract_thinking_trace(obj) else {
        return None;
    };
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
    let Some(tc) = extract_tool_call(obj) else {
        return None;
    };
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

fn handle_tool_result(
    obj: &serde_json::Map<String, Value>,
    sink: &mut SseControlSink<'_>,
) -> Option<SseDispatch> {
    let Some(parsed) = extract_tool_result(obj) else {
        return None;
    };
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
    let Some(log) = extract_timeline_log(obj) else {
        return None;
    };
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

    SseDispatch::Plain
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

    if key_present_non_null(obj, "staged_plan_started") {
        return Some(SseDispatch::Handled);
    }
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
            f(rev);
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

#[cfg(test)]
mod sse_control_order_tests {
    use std::fs;
    use std::path::PathBuf;

    use crabmate_sse_protocol::classify_sse_control_outcome;
    use serde_json::Value;

    use crate::i18n::Locale;

    use super::{
        SseClarifyTraceHooks, SseControlSink, SseDispatch, SseNoticeTimelineHooks,
        SseStagedPlanHooks, SseWorkspaceToolHooks, try_dispatch_sse_control_payload,
    };

    #[test]
    fn single_space_sse_payload_is_plain_not_handled() {
        assert_eq!(dispatch_triage_string(" "), "plain");
    }

    fn dispatch_triage_string(data: &str) -> &'static str {
        let mut on_err = |_msg: String| {};
        let mut sink = SseControlSink {
            user_locale: Locale::ZhHans,
            on_error: &mut on_err,
            workspace_tool: SseWorkspaceToolHooks {
                on_workspace_changed: None,
                on_tool_call: None,
                on_tool_status_change: None,
                on_parsing_tool_calls_change: None,
                on_tool_result: None,
                on_command_approval_request: None,
            },
            staged_plan: SseStagedPlanHooks {
                on_assistant_answer_phase: None,
                on_staged_plan_step_started: None,
                on_staged_plan_step_finished: None,
            },
            clarify_trace: SseClarifyTraceHooks {
                on_clarification_questionnaire: None,
                on_thinking_trace: None,
            },
            notice_timeline: SseNoticeTimelineHooks {
                on_conversation_saved_revision: None,
                on_timeline_log: None,
            },
        };
        match try_dispatch_sse_control_payload(data, &mut sink) {
            SseDispatch::Stop => "stop",
            SseDispatch::Handled => "handled",
            SseDispatch::Plain => "plain",
        }
    }

    /// 与共享 `classify_sse_control_outcome` 一致；与金样一致（`sse_capabilities` 版本不匹配时
    /// `try_dispatch` 可能额外 `Stop`，金样不覆盖该情形）。
    #[test]
    fn golden_sse_control_leptos_dispatch_matches_shared_classify() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("../fixtures/sse_control_golden.jsonl");
        let raw =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = t.splitn(3, '\t').collect();
            assert!(
                parts.len() == 3,
                "{}:{}: expected 3 tab columns",
                path.display(),
                line_no + 1
            );
            let json_line = parts[1].trim();
            let want = parts[2].trim();
            let v: Value = serde_json::from_str(json_line).unwrap_or_else(|e| {
                panic!(
                    "{}:{}: invalid json: {e}\n{json_line}",
                    path.display(),
                    line_no + 1
                )
            });
            let via_classify = classify_sse_control_outcome(&v);
            let via_dispatch = dispatch_triage_string(json_line);
            assert_eq!(
                via_classify,
                via_dispatch,
                "{}:{}: Leptos `try_dispatch` triage must match `crabmate-sse-protocol::classify_sse_control_outcome`\n  json: {json_line}",
                path.display(),
                line_no + 1
            );
            assert_eq!(
                via_dispatch,
                want,
                "{}:{}: dispatch triage must match golden fixture\n  json: {json_line}",
                path.display(),
                line_no + 1
            );
        }
    }
}
