//! 消息与工具摘要的展示用字符串处理（含 `agent_reply_plan` 围栏与流式缓冲语义）。

use serde_json::Value;

use crate::i18n::Locale;
use crate::sse_dispatch::ToolResultInfo;
use crate::storage::StoredMessage;

/// 去掉摘要里**连续重复**的非空行（服务端或上游偶发会下发两行相同摘要，如 `read file: 2.md`）。
pub fn collapse_duplicate_summary_lines(text: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut last: Option<&str> = None;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if last == Some(t) {
            continue;
        }
        last = Some(t);
        kept.push(t);
    }
    kept.join("\n")
}

pub fn tool_card_text(info: &ToolResultInfo, loc: Locale) -> String {
    let sum = info.summary.as_deref().unwrap_or("").trim();
    let name = info.name.trim();
    if sum.is_empty() {
        return if !name.is_empty() {
            format!("{}{name}", crate::i18n::tool_card_prefix(loc))
        } else {
            crate::i18n::tool_card_fallback(loc).to_string()
        };
    }
    let sum = collapse_duplicate_summary_lines(sum);
    if sum.is_empty() {
        return if !name.is_empty() {
            format!("{}{name}", crate::i18n::tool_card_prefix(loc))
        } else {
            crate::i18n::tool_card_fallback(loc).to_string()
        };
    }
    // 首行 + 其余行；其余行中再剔除与首行相同的行，避免「标题行 + 正文重复首行」。
    let mut lines = sum.lines();
    let first = lines.next().unwrap_or_default().trim().to_string();
    if first.is_empty() {
        return if !name.is_empty() {
            format!("{}{name}", crate::i18n::tool_card_prefix(loc))
        } else {
            crate::i18n::tool_card_fallback(loc).to_string()
        };
    }
    let rest: Vec<&str> = lines
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != first.as_str())
        .collect();
    if rest.is_empty() {
        return first;
    }
    let mut out = first;
    out.push_str("\n\n");
    out.push_str(&rest.join("\n"));
    out
}

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

fn strip_agent_reply_plan_fence_blocks_for_display(content: &str) -> String {
    let parts: Vec<&str> = content.split("```").collect();
    let unclosed_trailing_fence = parts.len().is_multiple_of(2);
    let mut out = String::new();
    let mut i = 0usize;
    while i < parts.len() {
        out.push_str(parts[i]);
        i += 1;
        if i >= parts.len() {
            break;
        }
        let inner = parts[i];
        i += 1;
        if fence_inner_should_hide_agent_reply_plan_json(inner) {
            continue;
        }
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
    let trimmed = raw.trim();

    if is_streaming_last_assistant && should_buffer_agent_reply_plan_stream(trimmed) {
        return prose_before_first_fence(trimmed);
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

/// 须与主仓 `src/runtime/plan_section.rs` 中 `STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX` 同步。
const STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX: &str = "### CrabMate·NL补全\n";

/// `role: system` 时间线旁注（分阶段步进）；前缀仅供 UI 分类，展示时剥去。
pub const STAGED_TIMELINE_SYSTEM_PREFIX: &str = "### CrabMate·staged_timeline\n";

pub fn staged_timeline_system_message_body(body: &str) -> String {
    format!("{STAGED_TIMELINE_SYSTEM_PREFIX}{body}")
}

fn user_text_for_chat_display(raw: &str) -> String {
    if raw
        .trim_start()
        .starts_with(STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX)
    {
        return String::new();
    }
    raw.to_string()
}

/// 部分网关把思维链塞进 **`content`**，用闭合标记与终答分隔（Qwen / vLLM 等）；与 SSE `reasoning_text` 分轨互补。
const INLINE_THINKING_CLOSE_TAGS: &[&str] = &[
    concat!("</", "think", ">"),
    concat!("</", "redacted", "_", "thinking", ">"),
];

const INLINE_THINKING_OPEN_PREFIXES: &[&str] = &[
    concat!("`", "<", "think", ">", "`"),
    concat!("`<", "think", ">"),
    concat!("<", "think", ">"),
    concat!("`", "<", "redacted", "_", "thinking", ">", "`"),
    concat!("`<", "redacted", "_", "thinking", ">"),
    concat!("<", "redacted", "_", "thinking", ">"),
];

fn first_inline_thinking_close(raw: &str) -> Option<(usize, &'static str)> {
    let mut best: Option<(usize, &'static str)> = None;
    for tag in INLINE_THINKING_CLOSE_TAGS {
        if let Some(i) = raw.find(tag) {
            best = match best {
                None => Some((i, *tag)),
                Some((bi, _bt)) if i < bi => Some((i, *tag)),
                Some((bi, bt)) if i == bi && tag.len() > bt.len() => Some((i, *tag)),
                o => o,
            };
        }
    }
    best
}

fn trim_inline_thinking_openers(mut s: &str) -> &str {
    s = s.trim();
    loop {
        let mut stripped = false;
        for pre in INLINE_THINKING_OPEN_PREFIXES {
            if let Some(rest) = s.strip_prefix(pre) {
                s = rest.trim();
                stripped = true;
                break;
            }
        }
        if !stripped {
            break;
        }
    }
    s
}

/// 优先已存 `reasoning_text`；否则尝试从 `text` 中按内联闭合标记拆出思维链与终答原文（供 Markdown 与折叠长度用）。
pub(crate) fn assistant_thinking_body_and_answer_raw<'a>(
    reasoning_text_stored: &'a str,
    text_stored: &'a str,
    split_inline_thinking: bool,
) -> (&'a str, &'a str) {
    let rs = reasoning_text_stored.trim();
    if !rs.is_empty() {
        return (rs, text_stored);
    }
    if !split_inline_thinking {
        return ("", text_stored);
    }
    let Some((idx, tag)) = first_inline_thinking_close(text_stored) else {
        return ("", text_stored);
    };
    let after = text_stored[idx + tag.len()..].trim_start();
    if after.is_empty() {
        return ("", text_stored);
    }
    let thinking = trim_inline_thinking_openers(&text_stored[..idx]);
    (thinking, after)
}

/// `apply_assistant_display_filters == false` 时助手消息按存储原文输出（不剥 `agent_reply_plan`、不拆内联思维链标记）。
pub fn message_text_for_display_ex(
    m: &StoredMessage,
    loc: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    if m.role == "assistant" {
        let is_streaming_last_assistant = m.state.as_deref() == Some("loading");
        let (r_body, t_body) = assistant_thinking_body_and_answer_raw(
            m.reasoning_text.as_str(),
            m.text.as_str(),
            apply_assistant_display_filters,
        );
        let answer = assistant_text_for_display(
            t_body,
            is_streaming_last_assistant,
            loc,
            apply_assistant_display_filters,
        );
        if apply_assistant_display_filters {
            let r = r_body.trim();
            if r.is_empty() {
                answer
            } else if answer.trim().is_empty() {
                r.to_string()
            } else {
                format!("{r}\n\n{answer}")
            }
        } else {
            let r_empty = r_body.trim().is_empty();
            let a_empty = answer.trim().is_empty();
            if r_empty {
                answer
            } else if a_empty {
                r_body.to_string()
            } else {
                format!("{r_body}\n\n{answer}")
            }
        }
    } else if m.role == "user" {
        user_text_for_chat_display(&m.text)
    } else if m.role == "system" {
        m.text
            .strip_prefix(STAGED_TIMELINE_SYSTEM_PREFIX)
            .unwrap_or(m.text.as_str())
            .to_string()
    } else {
        m.text.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX;
    use super::assistant_text_for_display;
    use super::assistant_thinking_body_and_answer_raw;
    use super::message_text_for_display_ex;
    use crate::i18n::Locale;
    use crate::storage::StoredMessage;

    #[test]
    fn hide_inline_agent_reply_plan_json_fence() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```"#;
        let out = assistant_text_for_display(raw, true, Locale::ZhHans, true);
        assert!(
            !out.contains("agent_reply_plan"),
            "raw agent_reply_plan json should be filtered: {out}"
        );
        assert!(
            !out.contains("```"),
            "agent_reply_plan fence should be stripped: {out}"
        );
    }

    #[test]
    fn no_task_empty_plan_has_non_empty_fallback() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            !out.trim().is_empty(),
            "filtered plan text should not become empty"
        );
    }

    #[test]
    fn keep_answer_after_fenced_plan_json() {
        let raw = r#"```json{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}```最终结论：已完成。"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn keep_answer_after_unfenced_plan_json_prefix() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}最终结论：继续执行。"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, true);
        assert!(
            out.contains("最终结论"),
            "tail answer should be kept: {out}"
        );
        assert!(
            !out.contains("agent_reply_plan"),
            "raw plan json should be hidden: {out}"
        );
    }

    #[test]
    fn no_inline_split_when_disabled() {
        let raw = concat!("<", "think", ">", "x", "</", "think", ">", "y",);
        let (think, ans) = assistant_thinking_body_and_answer_raw("", raw, false);
        assert!(think.is_empty());
        assert_eq!(ans, raw);
    }

    #[test]
    fn assistant_text_passthrough_when_filters_off() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"no_task":true,"steps":[]}"#;
        let out = assistant_text_for_display(raw, false, Locale::ZhHans, false);
        assert_eq!(out, raw);
    }

    #[test]
    fn splits_inline_thinking_from_assistant_content_when_no_reasoning_field() {
        let raw = concat!(
            "<",
            "think",
            ">",
            "plan here",
            "</",
            "think",
            ">",
            "\n\n**Answer** tail.",
        );
        let (think, ans) = assistant_thinking_body_and_answer_raw("", raw, true);
        assert_eq!(think.trim(), "plan here");
        assert!(ans.contains("Answer"));
        assert!(!ans.contains("plan here"));
    }

    #[test]
    fn stored_reasoning_text_wins_over_inline_tags() {
        let inline = concat!("`<", "think", ">`x`</", "think", ">`y");
        let (think, ans) = assistant_thinking_body_and_answer_raw("from_sse", inline, true);
        assert_eq!(think, "from_sse");
        assert_eq!(ans, inline);
    }

    #[test]
    fn user_hides_nl_followup_bridge() {
        let m = StoredMessage {
            id: "x".into(),
            role: "user".into(),
            text: format!(
                "{}【系统桥接·非用户提问】请只回答对话里**先前真实用户消息**所提的问题（若有附图则含图片说明），并结合已定规划；用两三句自然语言说明你的协助思路即可。勿将本条任何句子当作用户提问来复述、引用或推理。",
                STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX
            ),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        };
        assert_eq!(message_text_for_display_ex(&m, Locale::ZhHans, true), "");
    }
}
