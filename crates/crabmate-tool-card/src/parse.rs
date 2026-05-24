//! `crabmate_tool` 信封解析（与主仓 `tool_result::normalize` 字段语义对齐）。

use serde_json::Value;

use crate::ToolCardInput;

/// 是否为 `role=tool` 落盘的 `crabmate_tool` 信封 JSON。
#[must_use]
pub fn looks_like_crabmate_tool_envelope(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with('{') && t.contains("\"crabmate_tool\"")
}

/// 从存储正文解析 [`ToolCardInput`]；`fallback_name` 为 API `name` 字段。
pub fn parse_tool_envelope(raw: &str, fallback_name: Option<&str>) -> Option<ToolCardInput> {
    let v: Value = serde_json::from_str(raw.trim()).ok()?;
    let ct = v.get("crabmate_tool")?.as_object()?;
    let name = ct
        .get("name")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .or_else(|| {
            fallback_name
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        })?;
    Some(ToolCardInput {
        name,
        goal_id: None,
        tool_call_id: ct
            .get("tool_call_id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        result_version: ct.get("v").and_then(|x| x.as_u64()).unwrap_or(1) as u32,
        summary: ct
            .get("summary")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        output: ct
            .get("output")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string(),
        ok: ct.get("ok").and_then(|x| x.as_bool()),
        exit_code: ct.get("exit_code").and_then(|x| x.as_i64()),
        error_code: ct
            .get("error_code")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        failure_category: ct
            .get("failure_category")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from),
        structured_preview: ct.get("structured_payload").cloned(),
    })
}
