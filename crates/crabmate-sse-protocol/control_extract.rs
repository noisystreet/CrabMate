use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseErrorStop {
    pub message: String,
    pub code: String,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SseToolCall {
    pub name: String,
    pub summary: String,
    pub arguments_preview: Option<String>,
    pub arguments: Option<String>,
    pub goal_id: Option<String>,
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseToolOutputChunk {
    pub tool_call_id: String,
    pub name: Option<String>,
    pub seq: u64,
    pub chunk: String,
    pub stream: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SseToolResult {
    pub name: String,
    pub goal_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub result_version: u32,
    pub summary: Option<String>,
    pub output: String,
    pub ok: Option<bool>,
    pub exit_code: Option<i64>,
    pub error_code: Option<String>,
    pub failure_category: Option<String>,
    pub structured_preview: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseTimelineLog {
    pub kind: String,
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseStagedPlanStepStart {
    pub step_index: usize,
    pub total_steps: usize,
    pub description: String,
    pub executor_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseStagedPlanStepEnd {
    pub step_index: usize,
    pub total_steps: usize,
    pub status: String,
    pub executor_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseClarificationField {
    pub id: String,
    pub label: String,
    pub hint: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseClarificationQuestionnaire {
    pub questionnaire_id: String,
    pub intro: String,
    pub fields: Vec<SseClarificationField>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseThinkingTrace {
    pub op: String,
    pub node_id: Option<String>,
    pub parent_id: Option<String>,
    pub title: Option<String>,
    pub chunk: Option<String>,
    pub context_snapshot: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseTurnSegmentStart {
    pub segment_id: String,
    pub kind: String,
    pub before_tool_call_id: Option<String>,
}

pub fn extract_turn_segment_start(
    obj: &serde_json::Map<String, Value>,
) -> Option<SseTurnSegmentStart> {
    let t = obj.get("turn_segment_start")?.as_object()?;
    let segment_id = t
        .get("segment_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())?
        .to_string();
    let kind = t
        .get("kind")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("commentary")
        .to_string();
    let before_tool_call_id = t
        .get("before_tool_call_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    Some(SseTurnSegmentStart {
        segment_id,
        kind,
        before_tool_call_id,
    })
}

pub fn extract_turn_segment_end(obj: &serde_json::Map<String, Value>) -> Option<String> {
    let t = obj.get("turn_segment_end")?.as_object()?;
    t.get("segment_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

pub fn extract_tool_call(obj: &serde_json::Map<String, Value>) -> Option<SseToolCall> {
    let tc = obj.get("tool_call")?.as_object()?;
    let summary = tc.get("summary").and_then(|x| x.as_str()).unwrap_or("");
    let preview = tc
        .get("arguments_preview")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty());
    let args_full = tc
        .get("arguments")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty());
    if summary.is_empty() && preview.is_none() && args_full.is_none() {
        return None;
    }
    Some(SseToolCall {
        name: tc
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        summary: summary.to_string(),
        arguments_preview: preview.map(String::from),
        arguments: args_full.map(String::from),
        goal_id: tc
            .get("goal_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        tool_call_id: tc
            .get("tool_call_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
    })
}

pub fn extract_tool_output_chunk(
    obj: &serde_json::Map<String, Value>,
) -> Option<SseToolOutputChunk> {
    let ch = obj.get("tool_output_chunk")?.as_object()?;
    let tool_call_id = ch
        .get("tool_call_id")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())?
        .to_string();
    let seq = ch.get("seq").and_then(|x| x.as_u64())?;
    let chunk = ch
        .get("chunk")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let name = ch
        .get("name")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(String::from);
    let stream = ch
        .get("stream")
        .and_then(|x| x.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(String::from);
    Some(SseToolOutputChunk {
        tool_call_id,
        name,
        seq,
        chunk,
        stream,
    })
}

pub fn extract_tool_result(obj: &serde_json::Map<String, Value>) -> Option<SseToolResult> {
    let tr = obj.get("tool_result")?.as_object()?;
    if !(tr.get("output").is_some() || tr.get("structured_preview").is_some_and(|v| !v.is_null())) {
        return None;
    }
    Some(SseToolResult {
        name: tr
            .get("name")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        goal_id: tr
            .get("goal_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        tool_call_id: tr
            .get("tool_call_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
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
        failure_category: tr
            .get("failure_category")
            .and_then(|x| x.as_str())
            .map(String::from),
        structured_preview: tr.get("structured_preview").cloned(),
    })
}

pub fn extract_timeline_log(obj: &serde_json::Map<String, Value>) -> Option<SseTimelineLog> {
    let tl = obj.get("timeline_log")?.as_object()?;
    let kind = tl
        .get("kind")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let title = tl
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if kind.is_empty() && title.is_empty() {
        return None;
    }
    let detail = tl
        .get("detail")
        .and_then(|x| x.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    Some(SseTimelineLog {
        kind,
        title,
        detail,
    })
}

pub fn extract_error_stop(obj: &serde_json::Map<String, Value>) -> Option<SseErrorStop> {
    let error = obj.get("error")?;
    if error.is_null() {
        return None;
    }
    let code_raw = obj.get("code")?.as_str()?;
    let code = code_raw.trim();
    if code.is_empty() {
        return None;
    }
    let message = obj
        .get("error")
        .and_then(|x| x.as_str())
        .unwrap_or("error")
        .to_string();
    let reason_code = obj
        .get("reason_code")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    Some(SseErrorStop {
        message,
        code: code.to_string(),
        reason_code,
    })
}

pub fn extract_staged_plan_step_started(
    obj: &serde_json::Map<String, Value>,
) -> Option<SseStagedPlanStepStart> {
    let inner = obj.get("staged_plan_step_started")?.as_object()?;
    Some(SseStagedPlanStepStart {
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

pub fn extract_staged_plan_step_finished(
    obj: &serde_json::Map<String, Value>,
) -> Option<SseStagedPlanStepEnd> {
    let inner = obj.get("staged_plan_step_finished")?.as_object()?;
    Some(SseStagedPlanStepEnd {
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

pub fn extract_clarification_questionnaire(
    obj: &serde_json::Map<String, Value>,
) -> Option<SseClarificationQuestionnaire> {
    let inner = obj.get("clarification_questionnaire")?.as_object()?;
    let qid = inner
        .get("questionnaire_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)?;
    let intro = inner
        .get("intro")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)?;
    let qarr = inner.get("questions")?.as_array()?;
    let mut fields: Vec<SseClarificationField> = Vec::new();
    for q in qarr {
        let qo = match q.as_object() {
            Some(v) => v,
            None => continue,
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
        fields.push(SseClarificationField {
            id,
            label,
            hint,
            required,
        });
    }
    if fields.is_empty() {
        return None;
    }
    Some(SseClarificationQuestionnaire {
        questionnaire_id: qid,
        intro,
        fields,
    })
}

pub fn extract_thinking_trace(obj: &serde_json::Map<String, Value>) -> Option<SseThinkingTrace> {
    let tt = obj.get("thinking_trace")?.as_object()?;
    let op = tt
        .get("op")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .unwrap_or("");
    if op.is_empty() {
        return None;
    }
    Some(SseThinkingTrace {
        op: op.to_string(),
        node_id: tt
            .get("node_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        parent_id: tt
            .get("parent_id")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        title: tt
            .get("title")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        chunk: tt
            .get("chunk")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
        context_snapshot: tt
            .get("context_snapshot")
            .and_then(|x| x.as_str())
            .filter(|s| !s.is_empty())
            .map(String::from),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        extract_clarification_questionnaire, extract_error_stop, extract_staged_plan_step_finished,
        extract_staged_plan_step_started, extract_thinking_trace, extract_timeline_log,
        extract_tool_call, extract_tool_output_chunk, extract_tool_result,
    };
    use serde_json::json;

    #[test]
    fn extract_tool_call_requires_summary_or_args() {
        let v = json!({"tool_call":{"name":"read_file"}});
        let obj = v.as_object().unwrap();
        assert!(extract_tool_call(obj).is_none());
        let v = json!({"tool_call":{"name":"read_file","summary":"ok"}});
        let obj = v.as_object().unwrap();
        assert_eq!(extract_tool_call(obj).unwrap().name, "read_file");
    }

    #[test]
    fn extract_tool_output_chunk_requires_id_and_seq() {
        let v = json!({"tool_output_chunk":{"seq":1,"chunk":"x"}});
        let obj = v.as_object().unwrap();
        assert!(extract_tool_output_chunk(obj).is_none());
        let v = json!({"tool_output_chunk":{"tool_call_id":"tc","chunk":"x"}});
        let obj = v.as_object().unwrap();
        assert!(extract_tool_output_chunk(obj).is_none());
        let v = json!({"tool_output_chunk":{"tool_call_id":"tc","seq":2,"chunk":"ab"}});
        let obj = v.as_object().unwrap();
        let o = extract_tool_output_chunk(obj).unwrap();
        assert_eq!(o.chunk, "ab");
        assert_eq!(o.seq, 2);
        assert_eq!(o.tool_call_id, "tc");
    }

    #[test]
    fn extract_tool_result_requires_output_or_structured_preview() {
        let v = json!({"tool_result":{"name":"read_file"}});
        let obj = v.as_object().unwrap();
        assert!(extract_tool_result(obj).is_none());
        let v = json!({"tool_result":{"name":"read_file","output":"x"}});
        let obj = v.as_object().unwrap();
        assert_eq!(extract_tool_result(obj).unwrap().output, "x");
    }

    #[test]
    fn extract_timeline_log_requires_kind_or_title() {
        let v = json!({"timeline_log":{"kind":"","title":""}});
        let obj = v.as_object().unwrap();
        assert!(extract_timeline_log(obj).is_none());
        let v = json!({"timeline_log":{"kind":"k","title":"t"}});
        let obj = v.as_object().unwrap();
        assert_eq!(extract_timeline_log(obj).unwrap().kind, "k");
    }

    #[test]
    fn extract_error_stop_requires_non_empty_code() {
        let v = json!({"error":"x","code":"  "});
        let obj = v.as_object().unwrap();
        assert!(extract_error_stop(obj).is_none());
        let v = json!({"error":"x","code":"E_BAD","reason_code":"R1"});
        let obj = v.as_object().unwrap();
        let e = extract_error_stop(obj).unwrap();
        assert_eq!(e.code, "E_BAD");
        assert_eq!(e.reason_code.as_deref(), Some("R1"));
    }

    #[test]
    fn extract_staged_plan_steps() {
        let v =
            json!({"staged_plan_step_started":{"step_index":1,"total_steps":3,"description":"d"}});
        let obj = v.as_object().unwrap();
        assert_eq!(extract_staged_plan_step_started(obj).unwrap().step_index, 1);
        let v = json!({"staged_plan_step_finished":{"step_index":2,"total_steps":3,"status":"ok"}});
        let obj = v.as_object().unwrap();
        assert_eq!(extract_staged_plan_step_finished(obj).unwrap().status, "ok");
    }

    #[test]
    fn extract_clarification_requires_non_empty_fields() {
        let v = json!({
            "clarification_questionnaire":{
                "questionnaire_id":"q1",
                "intro":"i",
                "questions":[{"id":"f1","label":"L1","required":true}]
            }
        });
        let obj = v.as_object().unwrap();
        assert_eq!(
            extract_clarification_questionnaire(obj)
                .unwrap()
                .fields
                .len(),
            1
        );
    }

    #[test]
    fn extract_thinking_trace_requires_non_empty_op() {
        let v = json!({"thinking_trace":{"op":"  "}});
        let obj = v.as_object().unwrap();
        assert!(extract_thinking_trace(obj).is_none());
        let v = json!({"thinking_trace":{"op":"append","node_id":"n1"}});
        let obj = v.as_object().unwrap();
        assert_eq!(
            extract_thinking_trace(obj).unwrap().node_id.as_deref(),
            Some("n1")
        );
    }
}
