//! 控制面 JSON 的 **`stop` / `handled` / `plain`** 分类（与 Leptos `try_dispatch_sse_control_payload` 分支顺序同源）。
//!
//! **单一事实来源**：修改分支顺序时须同步 **`frontend-leptos/src/sse_dispatch.rs`**、本模块与
//! **`fixtures/sse_control_golden.jsonl`**；并跑 **`cargo test golden_sse_control`**。
//! 前端在 `sse_capabilities` 上可能因协议版本不匹配额外返回 `Stop`（本分类仍视为 `handled`，见
//! `try_dispatch` 内注释）；金样仅覆盖版本一致情形。

use serde_json::Value;

/// 顶层键存在且值非 `null`（与 Web / 镜像历史行为一致）。
pub fn key_present_non_null(obj: &serde_json::Map<String, Value>, key: &str) -> bool {
    match obj.get(key) {
        None | Some(Value::Null) => false,
        Some(_) => true,
    }
}

/// 返回 `"stop"` | `"handled"` | `"plain"`。
pub fn classify_sse_control_outcome(v: &Value) -> &'static str {
    let Some(obj) = v.as_object() else {
        return "plain";
    };

    // 与 TS：`parsed.error != null`（忽略 `error: null` 与缺省）
    if let Some(e) = obj.get("error")
        && !e.is_null()
        && let Some(Value::String(c)) = obj.get("code")
        && !c.trim().is_empty()
    {
        return "stop";
    }

    if obj.get("plan_required") == Some(&Value::Bool(true)) {
        return "handled";
    }

    if let Some(Value::Bool(_)) = obj.get("assistant_answer_phase") {
        return "handled";
    }

    if key_present_non_null(obj, "staged_plan_started") {
        return "handled";
    }
    if key_present_non_null(obj, "staged_plan_step_started") {
        return "handled";
    }
    if key_present_non_null(obj, "staged_plan_step_finished") {
        return "handled";
    }
    if key_present_non_null(obj, "staged_plan_finished") {
        return "handled";
    }

    if key_present_non_null(obj, "clarification_questionnaire") {
        return "handled";
    }

    if let Some(Value::Object(tt)) = obj.get("thinking_trace")
        && tt
            .get("op")
            .and_then(|x| x.as_str())
            .is_some_and(|s| !s.trim().is_empty())
    {
        return "handled";
    }

    if obj.get("workspace_changed") == Some(&Value::Bool(true)) {
        return "handled";
    }

    if let Some(Value::Object(tc)) = obj.get("tool_call") {
        let summary_ok = tc
            .get("summary")
            .and_then(|x| x.as_str())
            .is_some_and(|s| !s.is_empty());
        let preview_ok = tc
            .get("arguments_preview")
            .and_then(|x| x.as_str())
            .is_some_and(|s| !s.is_empty());
        let args_ok = tc
            .get("arguments")
            .and_then(|x| x.as_str())
            .is_some_and(|s| !s.is_empty());
        if summary_ok || preview_ok || args_ok {
            return "handled";
        }
    }

    if let Some(Value::Bool(_)) = obj.get("parsing_tool_calls") {
        return "handled";
    }
    if let Some(Value::Bool(_)) = obj.get("tool_running") {
        return "handled";
    }

    if let Some(Value::Object(tr)) = obj.get("tool_result")
        && tr.get("output").is_some()
    {
        return "handled";
    }

    if key_present_non_null(obj, "command_approval_request") {
        return "handled";
    }

    if obj.get("staged_plan_notice").is_some_and(|x| x.is_string())
        || obj.get("staged_plan_notice_clear") == Some(&Value::Bool(true))
    {
        return "handled";
    }

    if let Some(Value::Bool(_)) = obj.get("chat_ui_separator") {
        return "handled";
    }
    if key_present_non_null(obj, "conversation_saved") {
        return "handled";
    }

    if key_present_non_null(obj, "sse_capabilities") {
        return "handled";
    }
    if key_present_non_null(obj, "stream_ended") {
        return "handled";
    }
    if key_present_non_null(obj, "timeline_log") {
        return "handled";
    }

    "plain"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    /// 金样：每行 `描述<TAB>JSON<TAB>期望分类`（`stop`/`handled`/`plain`）。
    #[test]
    fn golden_sse_control_lines_match_typescript_contract() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("../../fixtures/sse_control_golden.jsonl");
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
                "{}:{}: expected 3 tab columns (line | json | outcome)",
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
            let got = classify_sse_control_outcome(&v);
            assert_eq!(
                got,
                want,
                "{}:{}: classify mismatch\n  json: {json_line}\n  want: {want}\n  got:  {got}",
                path.display(),
                line_no + 1
            );
        }
    }
}
