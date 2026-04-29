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
    use proptest::prelude::*;
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

    #[test]
    fn leptos_dispatch_branch_order_snapshot_stays_aligned() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = root.join("../../frontend-leptos/src/sse_dispatch.rs");
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));

        let checkpoints = [
            r#"if let Some(e) = obj.get("error")"#,
            r#"if obj.get("plan_required") == Some(&Value::Bool(true))"#,
            r#"if let Some(Value::Bool(b)) = obj.get("assistant_answer_phase")"#,
            r#"if key_present_non_null(obj, "staged_plan_started")"#,
            r#"if key_present_non_null(obj, "staged_plan_step_started")"#,
            r#"if key_present_non_null(obj, "staged_plan_step_finished")"#,
            r#"if key_present_non_null(obj, "staged_plan_finished")"#,
            r#"if key_present_non_null(obj, "clarification_questionnaire")"#,
            r#"if let Some(Value::Object(tt)) = obj.get("thinking_trace")"#,
            r#"if obj.get("workspace_changed") == Some(&Value::Bool(true))"#,
            r#"if let Some(Value::Object(tc)) = obj.get("tool_call")"#,
            r#"if let Some(Value::Bool(b)) = obj.get("parsing_tool_calls")"#,
            r#"if let Some(Value::Bool(b)) = obj.get("tool_running")"#,
            r#"if let Some(Value::Object(tr)) = obj.get("tool_result")"#,
            r#"if key_present_non_null(obj, "command_approval_request")"#,
            r#"if obj.get("staged_plan_notice").is_some_and(|x| x.is_string())"#,
            r#"if let Some(Value::Bool(_)) = obj.get("chat_ui_separator")"#,
            r#"if key_present_non_null(obj, "conversation_saved")"#,
            r#"if let Some(Value::Object(tl)) = obj.get("timeline_log")"#,
            r#"if key_present_non_null(obj, "sse_capabilities")"#,
            r#"if key_present_non_null(obj, "stream_ended")"#,
        ];

        let mut prev = 0usize;
        for needle in checkpoints {
            let idx = src.find(needle).unwrap_or_else(|| {
                panic!(
                    "{} must contain dispatch checkpoint: {}",
                    path.display(),
                    needle
                )
            });
            assert!(
                idx >= prev,
                "{} checkpoint out of order: {}",
                path.display(),
                needle
            );
            prev = idx;
        }
    }

    fn arb_non_empty_trimmed() -> impl Strategy<Value = String> {
        "[a-zA-Z0-9_\\- ]{1,32}"
            .prop_filter("must be non-empty after trim", |s| !s.trim().is_empty())
    }

    proptest! {
        #[test]
        fn prop_error_requires_non_empty_code_for_stop(
            error in proptest::option::of(arb_non_empty_trimmed()),
            code in proptest::option::of(" *"),
            extra_key in proptest::option::of(arb_non_empty_trimmed()),
        ) {
            let mut obj = serde_json::Map::new();
            if let Some(e) = error {
                obj.insert("error".to_string(), Value::String(e));
            }
            if let Some(c) = code {
                obj.insert("code".to_string(), Value::String(c));
            }
            if let Some(k) = extra_key {
                obj.insert(k, Value::Bool(true));
            }

            let got = classify_sse_control_outcome(&Value::Object(obj.clone()));
            let should_stop = obj
                .get("error")
                .is_some_and(|v| !v.is_null())
                && obj
                    .get("code")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.trim().is_empty());

            if should_stop {
                prop_assert_eq!(got, "stop");
            } else {
                prop_assert_ne!(got, "stop");
            }
        }

        #[test]
        fn prop_tool_call_fields_trigger_handled(
            summary in proptest::option::of(arb_non_empty_trimmed()),
            arguments_preview in proptest::option::of(arb_non_empty_trimmed()),
            arguments in proptest::option::of(arb_non_empty_trimmed()),
        ) {
            let mut tool_call = serde_json::Map::new();
            tool_call.insert("name".to_string(), Value::String("read_file".to_string()));
            if let Some(v) = &summary {
                tool_call.insert("summary".to_string(), Value::String(v.clone()));
            }
            if let Some(v) = &arguments_preview {
                tool_call.insert("arguments_preview".to_string(), Value::String(v.clone()));
            }
            if let Some(v) = &arguments {
                tool_call.insert("arguments".to_string(), Value::String(v.clone()));
            }
            let mut obj = serde_json::Map::new();
            obj.insert("tool_call".to_string(), Value::Object(tool_call));

            let got = classify_sse_control_outcome(&Value::Object(obj));
            let should_handle = summary.is_some() || arguments_preview.is_some() || arguments.is_some();
            if should_handle {
                prop_assert_eq!(got, "handled");
            } else {
                prop_assert_eq!(got, "plain");
            }
        }

        #[test]
        fn prop_stop_takes_precedence_over_other_handled_keys(
            code in arb_non_empty_trimmed(),
            plan_required in any::<bool>(),
            tool_running in any::<bool>(),
        ) {
            let payload = serde_json::json!({
                "v": 1,
                "error": "x",
                "code": code,
                "plan_required": plan_required,
                "tool_running": tool_running
            });
            prop_assert_eq!(classify_sse_control_outcome(&payload), "stop");
        }
    }
}
