//! `crabmate_timeline` 系统消息解析（从 `conversation_hydrate` 拆出以降低圈复杂度）。

use serde_json::Value;

fn tool_calls_summary(tool_calls: &Value) -> String {
    let Some(arr) = tool_calls.as_array() else {
        return String::new();
    };
    let mut lines: Vec<String> = Vec::new();
    for tc in arr {
        let Some(obj) = tc.as_object() else {
            continue;
        };
        let name = obj
            .get("function")
            .and_then(|f| f.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("?");
        lines.push(name.to_string());
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join(", ")
    }
}

fn first_tool_call_function_name(tool_calls: &Value) -> Option<String> {
    let arr = tool_calls.as_array()?;
    let tc = arr.first()?;
    let obj = tc.as_object()?;
    let n = obj.get("function")?.get("name")?.as_str()?.trim();
    if n.is_empty() {
        None
    } else {
        Some(n.to_string())
    }
}

use crate::i18n::load_locale_from_storage;
use crate::message_format::{
    format_tool_role_content_for_stored_message, tool_result_info_from_stored_content,
};
use crate::storage::StoredMessage;
use crate::timeline_scan::timeline_state_tool;

/// 将 `role=system` 且 `name=crabmate_timeline` 的一条 API 消息追加到 `out`（`t` 为单调 `created_at`）。
pub(crate) fn append_crabmate_timeline_system_message(
    body: &str,
    base_ms: i64,
    out: &mut Vec<StoredMessage>,
    t: &mut i64,
) {
    let body_trim = body.trim();
    append_generic_timeline_card(body_trim, base_ms, out, t);
}

fn append_generic_timeline_card(
    body_trim: &str,
    base_ms: i64,
    out: &mut Vec<StoredMessage>,
    t: &mut i64,
) {
    let id = format!("h_{}_{}", base_ms, out.len());
    *t = t.saturating_add(1);
    let display = body_trim.to_string();
    out.push(StoredMessage {
        id,
        role: "system".into(),
        text: display,
        reasoning_text: String::new(),
        image_urls: vec![],
        state: None,
        is_tool: false,
        tool_call_id: None,
        tool_name: None,
        created_at: *t,
    });
}

/// `assistant` 仅工具调用、无正文与思维链时，追加一条时间线式工具卡片。
pub(crate) fn append_assistant_tool_calls_timeline_card(
    parsed_tool_calls: &Value,
    base_ms: i64,
    out: &mut Vec<StoredMessage>,
    t: &mut i64,
) {
    let id = format!("h_{}_{}", base_ms, out.len());
    *t = t.saturating_add(1);
    let summary = tool_calls_summary(parsed_tool_calls);
    let card = if summary.is_empty() {
        "工具调用".to_string()
    } else {
        format!("工具：{summary}")
    };
    let state = timeline_state_tool(&id, true);
    let tool_name = first_tool_call_function_name(parsed_tool_calls);
    out.push(StoredMessage {
        id,
        role: "system".into(),
        text: card,
        reasoning_text: String::new(),
        image_urls: vec![],
        state: Some(state),
        is_tool: true,
        tool_call_id: None,
        tool_name,
        created_at: *t,
    });
}

/// `role=tool` 消息追加为时间线工具条目。
pub(crate) fn append_tool_role_timeline_row(
    name: &str,
    text: &str,
    display_content: Option<&str>,
    display_reasoning: Option<&str>,
    base_ms: i64,
    out: &mut Vec<StoredMessage>,
    t: &mut i64,
) {
    let id = format!("h_{}_{}", base_ms, out.len());
    *t = t.saturating_add(1);
    let fallback_name = name.trim();
    let fallback_name = (!fallback_name.is_empty()).then_some(fallback_name);
    let parsed = if display_content.is_some() || display_reasoning.is_some() {
        display_content.map(|c| (c.to_string(), display_reasoning.unwrap_or("").to_string()))
    } else {
        let loc = load_locale_from_storage();
        format_tool_role_content_for_stored_message(text, fallback_name, loc)
    };
    let tl_ok = tool_result_info_from_stored_content(text, fallback_name)
        .and_then(|info| info.ok)
        .unwrap_or(true);
    let state = timeline_state_tool(&id, tl_ok);
    let (display_text, reasoning_text, tool_call_id, tool_name) = match parsed {
        Some((compact, detail)) => {
            let info = tool_result_info_from_stored_content(text, fallback_name);
            (
                compact,
                detail,
                info.as_ref()
                    .and_then(|i| i.tool_call_id.clone())
                    .filter(|x| !x.trim().is_empty()),
                info.map(|i| i.name)
                    .or_else(|| fallback_name.map(String::from)),
            )
        }
        None => (
            text.to_string(),
            String::new(),
            None,
            fallback_name.map(String::from),
        ),
    };
    out.push(StoredMessage {
        id,
        role: "system".into(),
        text: display_text,
        reasoning_text,
        image_urls: vec![],
        state: Some(state),
        is_tool: true,
        tool_call_id,
        tool_name,
        created_at: *t,
    });
}
