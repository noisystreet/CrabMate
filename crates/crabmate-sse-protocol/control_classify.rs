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
        "staged_plan_step_started",
        "staged_plan_step_finished",
        "turn_segment_start",
        "turn_segment_end",
        "tool_call",
        "tool_output_chunk",
        "tool_result",
        "timeline_log",
        "thinking_trace",
        "conversation_saved",
        "staged_plan_started",
        "staged_plan_finished",
        "staged_plan_notice",
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
