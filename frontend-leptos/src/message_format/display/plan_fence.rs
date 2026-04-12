//! `agent_reply_plan` JSON、``` 围栏剥离与助手正文「规划轮」展示。

use serde_json::Value;

use crate::i18n::Locale;
use crate::storage::StoredMessage;

use super::thinking_strip::filter_assistant_thinking_markers_for_display;

fn format_agent_reply_plan_json_for_display(
    json_text: &str,
    goal: &str,
    loc: Locale,
) -> Option<String> {
    let v: Value = serde_json::from_str(json_text).ok()?;
    let obj = v.as_object()?;
    if obj.get("type").and_then(|x| x.as_str()) != Some("agent_reply_plan") {
        return None;
    }
    let steps = obj.get("steps").and_then(|x| x.as_array())?;

    let mut lines = Vec::with_capacity(steps.len().saturating_add(1));
    let goal = goal.trim();
    if !goal.is_empty() {
        lines.push(goal.to_string());
    }
    if steps.is_empty() {
        if !goal.is_empty() {
            return Some(goal.to_string());
        }
        return Some(crate::i18n::plan_generated(loc).to_string());
    }
    if !goal.is_empty() {
        lines.push(String::new());
    }
    for (idx, s) in steps.iter().enumerate() {
        let id = s
            .get("id")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .unwrap_or(crate::i18n::plan_step_placeholder_id());
        let desc = s
            .get("description")
            .and_then(|x| x.as_str())
            .filter(|x| !x.trim().is_empty())
            .unwrap_or(crate::i18n::plan_step_no_desc(loc));
        lines.push(crate::i18n::plan_step_line(
            loc,
            idx,
            id.trim(),
            desc.trim(),
        ));
    }
    Some(lines.join("\n"))
}

fn fenced_body_after_optional_jsonish_lang_label(inner: &str) -> Option<&str> {
    let s = inner.trim_start_matches(['\n', '\r', ' ', '\t']);
    if s.is_empty() {
        return Some("");
    }
    for label in ["json", "markdown", "md"] {
        if let Some(rest) = s.strip_prefix(label) {
            let mut chars = rest.chars();
            let next = chars.next();
            // 兼容两种形态：
            // 1) ```json\n{...}
            // 2) ```json{...}
            if next.is_none()
                || next == Some('\n')
                || next == Some('\r')
                || next == Some(' ')
                || next == Some('\t')
                || next == Some('{')
                || next == Some('[')
            {
                return Some(rest.trim_start_matches(['\n', '\r', ' ', '\t']));
            }
        }
    }
    None
}

fn triple_backtick_fence_count(s: &str) -> usize {
    s.match_indices("```").count()
}

fn first_fence_inner_looks_like_json_object(s: &str) -> bool {
    let mut it = s.split("```");
    let _ = it.next();
    let Some(inner) = it.next() else {
        return false;
    };
    let Some(body) = fenced_body_after_optional_jsonish_lang_label(inner) else {
        return false;
    };
    let b = body.trim();
    b.is_empty() || b.starts_with('{')
}

fn looks_like_incomplete_agent_reply_plan_whole_json(t: &str) -> bool {
    let t = t.trim();
    if !t.starts_with('{') {
        return false;
    }
    if t.contains("\"agent_reply_plan\"") {
        return true;
    }
    t.contains("\"type\"") && t.contains("\"version\"") && t.contains("\"steps\"")
}

/// Web 气泡眉「规划轮」标记：分阶段规划模型产出在 `text` / `reasoning_text` 中含 `agent_reply_plan`（含流式不完整前缀）。
pub(crate) fn stored_message_is_staged_planner_round(m: &StoredMessage) -> bool {
    if m.role != "assistant" || m.is_tool {
        return false;
    }
    let raw = format!("{}{}", m.reasoning_text, m.text);
    if raw.contains("\"agent_reply_plan\"") || raw.contains("\"type\":\"agent_reply_plan\"") {
        return true;
    }
    looks_like_incomplete_agent_reply_plan_whole_json(raw.trim())
}

fn should_buffer_agent_reply_plan_stream(stripped: &str) -> bool {
    if triple_backtick_fence_count(stripped) % 2 == 1
        && first_fence_inner_looks_like_json_object(stripped)
    {
        return true;
    }
    let t = stripped.trim();
    if !t.starts_with('{') {
        return false;
    }
    if format_agent_reply_plan_json_for_display(t, "", Locale::ZhHans).is_some() {
        return false;
    }
    serde_json::from_str::<Value>(t).is_err()
        && looks_like_incomplete_agent_reply_plan_whole_json(t)
}

fn prose_before_first_fence(s: &str) -> String {
    s.split("```").next().unwrap_or("").trim().to_string()
}

fn fence_inner_should_hide_agent_reply_plan_json(inner: &str) -> bool {
    let raw = inner.trim();
    let body = fenced_body_after_optional_jsonish_lang_label(raw)
        .unwrap_or(raw)
        .trim();
    if !body.starts_with('{') {
        return false;
    }
    if format_agent_reply_plan_json_for_display(body, "", Locale::ZhHans).is_some() {
        return true;
    }
    if !body.contains("\"agent_reply_plan\"") || !body.contains("\"steps\"") {
        return false;
    }
    serde_json::from_str::<Value>(body).is_ok()
}

/// 首段围栏前文本是否可视为「仅规划说明」从而省略，避免误删「大段正文 + 文末 plan 围栏」。
fn drop_first_segment_before_hidden_agent_reply_plan_fence(segment: &str) -> bool {
    let t = segment.trim();
    if t.is_empty() {
        return true;
    }
    if t.contains("\n## ") || t.contains("\n### ") || t.starts_with("## ") || t.starts_with("### ")
    {
        return false;
    }
    true
}

fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
    let unclosed_trailing_fence = parts.len().is_multiple_of(2);
    let mut out = String::new();
    let mut i = 0usize;
    let mut is_first_code_fence = true;
    while i < parts.len() {
        let segment = parts[i];
        i += 1;
        if i >= parts.len() {
            out.push_str(segment);
            break;
        }
        let inner = parts[i];
        i += 1;
        if fence_inner_should_hide_agent_reply_plan_json(inner) {
            let skip_segment = is_first_code_fence
                && drop_first_segment_before_hidden_agent_reply_plan_fence(segment);
            if !skip_segment {
                out.push_str(segment);
            }
            is_first_code_fence = false;
            continue;
        }
        is_first_code_fence = false;
        out.push_str(segment);
        if unclosed_trailing_fence && i >= parts.len() && inner.trim().is_empty() {
            break;
        }
        out.push_str("```");
        out.push_str(inner);
        out.push_str("```");
    }
    out
}

pub(crate) fn assistant_text_for_display(
    raw: &str,
    is_streaming_last_assistant: bool,
    loc: Locale,
    apply_filters: bool,
) -> String {
    if !apply_filters {
        return raw.to_string();
    }
    let inner = assistant_text_for_display_inner(raw, is_streaming_last_assistant, loc);
    filter_assistant_thinking_markers_for_display(&inner, is_streaming_last_assistant)
}

fn assistant_text_for_display_inner(
    raw: &str,
    is_streaming_last_assistant: bool,
    loc: Locale,
) -> String {
    let trimmed = raw.trim();

    if is_streaming_last_assistant && should_buffer_agent_reply_plan_stream(trimmed) {
        // 须与 `assistant_text_for_display` 外套的 `filter_assistant_thinking_markers_for_display` 一致。
        return filter_assistant_thinking_markers_for_display(
            &prose_before_first_fence(trimmed),
            true,
        );
    }

    if let Some(display) = format_agent_reply_plan_json_for_display(trimmed, "", loc)
        && !display.trim().is_empty()
    {
        return display;
    }

    // 无围栏但以前缀 JSON 输出规划：去掉前缀规划对象，保留后续终答正文。
    let t = raw.trim_start();
    if t.starts_with('{') && t.contains("\"agent_reply_plan\"") {
        let mut de = serde_json::Deserializer::from_str(t).into_iter::<Value>();
        if let Some(Ok(v)) = de.next()
            && v.as_object()
                .and_then(|o| o.get("type"))
                .and_then(|x| x.as_str())
                == Some("agent_reply_plan")
        {
            let offset = de.byte_offset();
            if offset < t.len() {
                let tail = t[offset..].trim();
                if !tail.is_empty() {
                    return tail.to_string();
                }
            }
        }
    }

    // 再做一次全量围栏剥离兜底：无论 `agent_reply_plan` 围栏出现在第几个代码块，都不回显原始 JSON。
    let stripped_fences = strip_agent_reply_plan_fence_blocks_for_display(raw);
    let stripped_trim = stripped_fences.trim();
    if stripped_trim != trimmed {
        if stripped_trim.is_empty() && raw.contains("\"agent_reply_plan\"") {
            return crate::i18n::plan_generated(loc).to_string();
        }
        return stripped_trim.to_string();
    }

    raw.to_string()
}
