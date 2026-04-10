//! 前端 SSE 控制面 JSON 的分类与分发（`serde_json::Value`）。
//! 分支顺序须与 `src/sse/control_dispatch_mirror.rs` 与 `fixtures/sse_control_golden.jsonl` 一致。

use crabmate_sse_protocol::SSE_PROTOCOL_VERSION;
use serde_json::Value;

fn key_present_non_null(obj: &serde_json::Map<String, Value>, key: &str) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => false,
        Some(_) => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SseDispatch {
    Stop,
    Handled,
    Plain,
}

pub struct SseCallbacks<'a> {
    pub on_error: &'a mut dyn FnMut(String),
    pub on_workspace_changed: Option<&'a mut dyn FnMut()>,
    pub on_tool_call: Option<&'a mut dyn FnMut(String, String)>,
    pub on_tool_status_change: Option<&'a mut dyn FnMut(bool)>,
    pub on_parsing_tool_calls_change: Option<&'a mut dyn FnMut(bool)>,
    pub on_tool_result: Option<&'a mut dyn FnMut(ToolResultInfo)>,
    pub on_command_approval_request: Option<&'a mut dyn FnMut(CommandApprovalRequest)>,
    /// `conversation_saved.revision`，供 `POST /chat/branch` 与冲突检测。
    pub on_conversation_saved_revision: Option<&'a mut dyn FnMut(u64)>,
    pub on_staged_plan_step_started: Option<&'a mut dyn FnMut(StagedPlanStepStartInfo)>,
    pub on_staged_plan_step_finished: Option<&'a mut dyn FnMut(StagedPlanStepEndInfo)>,
    pub on_clarification_questionnaire: Option<&'a mut dyn FnMut(ClarificationQuestionnaireInfo)>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // 与后端 JSON 同形；展示层当前仅用 name/summary。
pub struct ToolResultInfo {
    pub name: String,
    /// 与 `crabmate_tool.v` 对齐；缺省按 **1**（与后端 `serde(default)` 一致）。
    pub result_version: u32,
    pub summary: Option<String>,
    pub output: String,
    pub ok: Option<bool>,
    pub exit_code: Option<i64>,
    pub error_code: Option<String>,
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

fn parse_staged_plan_step_started(
    obj: &serde_json::Map<String, Value>,
) -> Option<StagedPlanStepStartInfo> {
    let inner = obj.get("staged_plan_step_started")?.as_object()?;
    Some(StagedPlanStepStartInfo {
        step_index: inner
            .get("step_index")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as usize,
        total_steps: inner
            .get("total_steps")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as usize,
        description: inner
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        executor_kind: inner
            .get("executor_kind")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
    })
}

fn parse_staged_plan_step_finished(
    obj: &serde_json::Map<String, Value>,
) -> Option<StagedPlanStepEndInfo> {
    let inner = obj.get("staged_plan_step_finished")?.as_object()?;
    Some(StagedPlanStepEndInfo {
        step_index: inner
            .get("step_index")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as usize,
        total_steps: inner
            .get("total_steps")
            .and_then(|x| x.as_u64())
            .unwrap_or(0) as usize,
        status: inner
            .get("status")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        executor_kind: inner
            .get("executor_kind")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
    })
}

/// 解析 `data:` 行内容（已去掉 `data: ` 前缀）；非 JSON 或解析失败时返回 `Plain`。
pub fn try_dispatch_sse_control_payload(data: &str, cbs: &mut SseCallbacks<'_>) -> SseDispatch {
    let Ok(v) = serde_json::from_str::<Value>(data) else {
        return SseDispatch::Plain;
    };
    let Some(obj) = v.as_object() else {
        return SseDispatch::Plain;
    };

    if let Some(e) = obj.get("error")
        && !e.is_null()
        && let Some(Value::String(code)) = obj.get("code")
        && !code.trim().is_empty()
    {
        let msg = obj.get("error").and_then(|x| x.as_str()).unwrap_or("error");
        let code = code.trim();
        let reason = obj
            .get("reason_code")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let line = match reason {
            Some(r) => format!("{msg} ({code}, reason_code={r})"),
            None => format!("{msg} ({code})"),
        };
        (cbs.on_error)(line);
        return SseDispatch::Stop;
    }

    if obj.get("plan_required") == Some(&Value::Bool(true)) {
        return SseDispatch::Handled;
    }
    if key_present_non_null(obj, "staged_plan_started") {
        return SseDispatch::Handled;
    }
    if key_present_non_null(obj, "staged_plan_step_started") {
        if let Some(info) = parse_staged_plan_step_started(obj)
            && let Some(f) = cbs.on_staged_plan_step_started.as_mut()
        {
            f(info);
        }
        return SseDispatch::Handled;
    }
    if key_present_non_null(obj, "staged_plan_step_finished") {
        if let Some(info) = parse_staged_plan_step_finished(obj)
            && let Some(f) = cbs.on_staged_plan_step_finished.as_mut()
        {
            f(info);
        }
        return SseDispatch::Handled;
    }
    if key_present_non_null(obj, "staged_plan_finished") {
        return SseDispatch::Handled;
    }

    if key_present_non_null(obj, "clarification_questionnaire") {
        if let Some(Value::Object(inner)) = obj.get("clarification_questionnaire")
            && let Some(qid) = inner
                .get("questionnaire_id")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
            && let Some(intro) = inner
                .get("intro")
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
            && let Some(Value::Array(qarr)) = inner.get("questions")
        {
            let mut fields: Vec<ClarificationFormField> = Vec::new();
            for q in qarr {
                let Some(qo) = q.as_object() else {
                    continue;
                };
                let id = qo
                    .get("id")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let label = qo
                    .get("label")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let (Some(id), Some(label)) = (id, label) else {
                    continue;
                };
                let hint = qo
                    .get("hint")
                    .and_then(|x| x.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(String::from);
                let required = qo
                    .get("required")
                    .and_then(|x| x.as_bool())
                    .unwrap_or(false);
                fields.push(ClarificationFormField {
                    id,
                    label,
                    hint,
                    required,
                });
            }
            if !fields.is_empty()
                && let Some(f) = cbs.on_clarification_questionnaire.as_mut()
            {
                f(ClarificationQuestionnaireInfo {
                    questionnaire_id: qid,
                    intro,
                    fields,
                });
            }
        }
        return SseDispatch::Handled;
    }

    if obj.get("workspace_changed") == Some(&Value::Bool(true)) {
        if let Some(f) = cbs.on_workspace_changed.as_mut() {
            f();
        }
        return SseDispatch::Handled;
    }

    if let Some(Value::Object(tc)) = obj.get("tool_call")
        && let Some(Value::String(s)) = tc.get("summary")
        && !s.is_empty()
    {
        let name = tc
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if let Some(f) = cbs.on_tool_call.as_mut() {
            f(name, s.clone());
        }
        return SseDispatch::Handled;
    }

    if let Some(Value::Bool(b)) = obj.get("parsing_tool_calls") {
        if let Some(f) = cbs.on_parsing_tool_calls_change.as_mut() {
            f(*b);
        }
        return SseDispatch::Handled;
    }
    if let Some(Value::Bool(b)) = obj.get("tool_running") {
        if let Some(f) = cbs.on_tool_status_change.as_mut() {
            f(*b);
        }
        return SseDispatch::Handled;
    }

    if let Some(Value::Object(tr)) = obj.get("tool_result")
        && tr.get("output").is_some()
    {
        let info = ToolResultInfo {
            name: tr
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            result_version: tr
                .get("result_version")
                .and_then(|x| x.as_u64())
                .map(|u| u as u32)
                .unwrap_or(1),
            summary: tr.get("summary").and_then(|x| x.as_str()).map(String::from),
            output: tr
                .get("output")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            ok: tr.get("ok").and_then(|x| x.as_bool()),
            exit_code: tr.get("exit_code").and_then(|x| x.as_i64()),
            error_code: tr
                .get("error_code")
                .and_then(|x| x.as_str())
                .map(String::from),
        };
        if let Some(f) = cbs.on_tool_result.as_mut() {
            f(info);
        }
        return SseDispatch::Handled;
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
            if let Some(f) = cbs.on_command_approval_request.as_mut() {
                f(req);
            }
        }
        return SseDispatch::Handled;
    }

    if obj.get("staged_plan_notice").is_some_and(|x| x.is_string())
        || obj.get("staged_plan_notice_clear") == Some(&Value::Bool(true))
    {
        return SseDispatch::Handled;
    }

    if let Some(Value::Bool(_)) = obj.get("chat_ui_separator") {
        return SseDispatch::Handled;
    }
    if key_present_non_null(obj, "conversation_saved") {
        if let Some(Value::Object(saved)) = obj.get("conversation_saved")
            && let Some(rev) = saved.get("revision").and_then(|x| x.as_u64())
            && let Some(f) = cbs.on_conversation_saved_revision.as_mut()
        {
            f(rev);
        }
        return SseDispatch::Handled;
    }

    if key_present_non_null(obj, "sse_capabilities") {
        if let Some(Value::Object(caps)) = obj.get("sse_capabilities")
            && let Some(sv_raw) = caps.get("supported_sse_v")
        {
            let sv = sv_raw
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .or_else(|| sv_raw.as_i64().and_then(|n| u8::try_from(n).ok()));
            if let Some(sv) = sv {
                if sv != SSE_PROTOCOL_VERSION {
                    let hint = if sv > SSE_PROTOCOL_VERSION {
                        "SSE_SERVER_TOO_NEW"
                    } else {
                        "SSE_SERVER_TOO_OLD"
                    };
                    (cbs.on_error)(format!(
                        "SSE 协议版本不匹配：服务端 supported_sse_v={sv}，本页 {SSE_PROTOCOL_VERSION} ({hint})"
                    ));
                    return SseDispatch::Stop;
                }
            }
        }
        return SseDispatch::Handled;
    }
    if key_present_non_null(obj, "stream_ended") {
        return SseDispatch::Handled;
    }

    SseDispatch::Plain
}
