//! SSE 控制面 JSON 的 `stop`/`handled`/`plain` 分类（V1 遗留接口；IM bridge 等外部集成仍使用）。

use serde_json::Value;

/// 检查 JSON 对象中某键存在且非 `null`。
pub fn key_present_non_null(obj: &serde_json::Map<String, Value>, key: &str) -> bool {
    obj.get(key).is_some_and(|v| !v.is_null())
}

/// 对已解析的 JSON 值做 `stop`/`handled`/`plain` 三态分类。
pub fn classify_sse_control_outcome(v: &Value) -> &'static str {
    let Some(obj) = v.as_object() else {
        return "plain";
    };

    // ── 停止条件 ──
    if key_present_non_null(obj, "error") {
        return "stop";
    }

    // ── 处理条件（非 null 存在性检查）──
    const NON_NULL_KEYS: &[&str] = &[
        "command_approval_request",
        "clarification_questionnaire",
        "assistant_answer_phase",
        "turn_segment_start",
        "turn_segment_end",
        "tool_call",
        "tool_output_chunk",
        "tool_result",
        "timeline_log",
        "thinking_trace",
        "conversation_saved",
        "sse_capabilities",
    ];
    for key in NON_NULL_KEYS {
        if key_present_non_null(obj, key) {
            return "handled";
        }
    }

    // ── 处理条件（布尔值检查）──
    const BOOL_TRUE_KEYS: &[&str] = &[
        "turn_tool_phase_end",
        "tool_running",
        "parsing_tool_calls",
        "workspace_changed",
    ];
    for key in BOOL_TRUE_KEYS {
        if v.get(key) == Some(&Value::Bool(true)) {
            return "handled";
        }
    }

    // `tool_running: false` 和 `parsing_tool_calls: false` 也视为 handled。
    if v.get("tool_running") == Some(&Value::Bool(false))
        || v.get("parsing_tool_calls") == Some(&Value::Bool(false))
    {
        return "handled";
    }

    // `chat_ui_separator` 任意布尔值均视为 handled。
    if v.get("chat_ui_separator")
        .and_then(|x| x.as_bool())
        .is_some()
    {
        return "handled";
    }

    // `v` 顶层有正常值（`{"v":1}` / `{"v":2}`） → handled（协议确认）。
    if v.get("v").and_then(|x| x.as_u64()).is_some() {
        return "handled";
    }

    "plain"
}

#[cfg(test)]
mod tests {
    use super::classify_sse_control_outcome;
    use serde_json::Value;
    use std::path::PathBuf;

    #[test]
    fn golden_sse_control() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sse_control_golden.jsonl");
        let raw = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        for (line_no, line) in raw.lines().enumerate() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = t.splitn(3, '\t').collect();
            assert_eq!(
                parts.len(),
                3,
                "{}:{}: expected 3 tab columns",
                path.display(),
                line_no + 1
            );
            let json_line = parts[1].trim();
            let want = parts[2].trim();
            let v: Value = serde_json::from_str(json_line).unwrap_or_else(|e| {
                panic!(
                    "{}:{}: invalid JSON ({e}): {json_line}",
                    path.display(),
                    line_no + 1
                )
            });
            let got = classify_sse_control_outcome(&v);
            assert_eq!(
                got,
                want,
                "{}:{}: classify mismatch\n  desc: {}\n  json: {json_line}\n  want: {want}\n  got:  {got}",
                path.display(),
                line_no + 1,
                parts[0],
            );
        }
    }
}
