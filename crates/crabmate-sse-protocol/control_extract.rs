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

#[cfg(test)]
mod tests {
    use super::{extract_error_stop, extract_timeline_log, extract_tool_call, extract_tool_result};
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
}
