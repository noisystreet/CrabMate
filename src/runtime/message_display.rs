//! 对话消息在 UI/终端上的展示用正文（与 `Message.content` 存储形态解耦）。

use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use crate::agent::plan_artifact::{
    augment_agent_reply_plan_goal_for_display, format_agent_reply_plan_for_display,
    parse_agent_reply_plan_v1, prose_before_first_fence,
    strip_agent_reply_plan_fence_blocks_for_display,
};
use crate::runtime::latex_unicode::latex_math_to_unicode;
use crate::tool_result::ToolResult;
use crate::types::Message;

/// 聊天区（TUI / Web 工具卡）是否展示 **【执行结果】** 整块（状态行、stdout/stderr、完整 JSON、纯文本正文等）。
/// `false` 时仅展示 **【描述与总结】** / JSON `human_summary` 等摘要；**不打印**「【执行结果】」及其下任何内容；`Message.content` 与 tracing 仍为全文。
pub(crate) const SHOW_TOOL_RAW_OUTPUT_IN_CHAT: bool = false;

/// `role: tool` 的展示：与 Web `ChatPanel` 的 `buildToolOutputCardText` 对齐。
/// [`SHOW_TOOL_RAW_OUTPUT_IN_CHAT`] 为 `false` 时仅 JSON `human_summary` 等摘要，**无**「【执行结果】」；
/// 为 `true` 时：先 `human_summary`，再 **【执行结果】**（状态 + stdout/stderr 等）；纯文本 `run_command` 风格则结构化展示。
///
/// 受 [`SHOW_TOOL_RAW_OUTPUT_IN_CHAT`] 控制；CLI 无 SSE 回显请用 [`tool_content_for_display_full`]。
pub(crate) fn tool_content_for_display(raw: &str) -> String {
    tool_content_for_display_impl(raw, SHOW_TOOL_RAW_OUTPUT_IN_CHAT)
}

/// 终端 CLI 等需与「聊天区省略策略」独立时：始终包含完整工具输出（与日志/对话历史一致）。
pub(crate) fn tool_content_for_display_full(raw: &str) -> String {
    tool_content_for_display_impl(raw, true)
}

pub(crate) fn tool_content_for_display_impl(raw: &str, include_raw: bool) -> String {
    let t = raw.trim();
    if t.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(t)
    {
        if include_raw {
            if let Some(h) = v.get("human_summary").and_then(|x| x.as_str()) {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| t.to_string());
                return format!("{h}\n\n【执行结果】\n{pretty}");
            }
            return serde_json::to_string_pretty(&v).unwrap_or_else(|_| t.to_string());
        }
        if let Some(h) = v.get("human_summary").and_then(|x| x.as_str()) {
            let hs = h.trim();
            if hs.is_empty() {
                return String::new();
            }
            return hs.to_string();
        }
        return String::new();
    }
    if should_format_as_structured_plain_tool(t) {
        return format_structured_plain_tool(t, include_raw);
    }
    if include_raw {
        t.to_string()
    } else {
        String::new()
    }
}

fn should_format_as_structured_plain_tool(raw: &str) -> bool {
    let first = raw.lines().next().unwrap_or("").trim();
    if first.starts_with("退出码：") {
        return true;
    }
    if first.contains("(exit=") && first.contains(')') {
        return true;
    }
    raw.contains("标准输出：\n") || raw.contains("标准错误：\n")
}

fn strip_first_tool_status_line(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let first = lines[0].trim();
    if first.starts_with("退出码：") || (first.contains("(exit=") && first.contains(')')) {
        return lines[1..].join("\n").trim().to_string();
    }
    raw.to_string()
}

fn format_structured_plain_tool(raw: &str, include_raw: bool) -> String {
    if !include_raw {
        return String::new();
    }
    let structured = ToolResult::from_legacy_output("tool", raw.to_string());
    let mut status_parts = Vec::new();
    status_parts.push(if structured.ok {
        "成功".to_string()
    } else {
        "失败".to_string()
    });
    if let Some(c) = structured.exit_code {
        status_parts.push(format!("exit={c}"));
    }
    if let Some(ref e) = structured.error_code {
        status_parts.push(format!("code={e}"));
    }
    let status_line = format!("状态：{}", status_parts.join(" | "));

    let result_body = if !structured.stdout.is_empty() || !structured.stderr.is_empty() {
        let mut chunks = Vec::new();
        if !structured.stdout.is_empty() {
            chunks.push(format!("标准输出：\n{}", structured.stdout));
        }
        if !structured.stderr.is_empty() {
            chunks.push(format!("标准错误：\n{}", structured.stderr));
        }
        chunks.join("\n\n")
    } else {
        let rest = strip_first_tool_status_line(raw);
        if rest.trim().is_empty() {
            "(无)".to_string()
        } else {
            rest
        }
    };

    format!("【执行结果】\n{status_line}\n{result_body}")
}

/// 根据对条 `assistant.tool_calls` 解析 `summarize_tool_call`（与 Web SSE `tool_result.summary` 同源）。
fn find_tool_call_for_display(messages: &[Message], tool_idx: usize) -> Option<(String, String)> {
    let tid = messages.get(tool_idx)?.tool_call_id.as_deref()?;
    for j in (0..tool_idx).rev() {
        let a = &messages[j];
        if a.role != "assistant" {
            continue;
        }
        let calls = a.tool_calls.as_ref()?;
        for c in calls {
            if c.id == tid {
                return Some((c.function.name.clone(), c.function.arguments.clone()));
            }
        }
    }
    None
}

/// TUI 行缓存指纹：同一条 `tool` 消息在「assistant 已带上 tool_calls」前后，展示可能多出一节「描述与总结」。
pub(crate) fn tool_display_context_fingerprint(messages: &[Message], tool_msg_idx: usize) -> u64 {
    let mut h = DefaultHasher::new();
    if let Some((name, args)) = find_tool_call_for_display(messages, tool_msg_idx) {
        name.hash(&mut h);
        args.hash(&mut h);
    }
    h.finish()
}

/// **TUI / 导出**：在 [`tool_content_for_display`] 之上，为 `role: tool` 追加与 Web 一致的「描述与总结」
///（来自 `summarize_tool_call`，依赖历史中**对条** assistant 的 `tool_calls`）。
pub(crate) fn tool_content_for_display_for_message(
    raw: &str,
    messages: &[Message],
    tool_msg_idx: usize,
) -> String {
    let body = tool_content_for_display(raw);
    let Some((name, args)) = find_tool_call_for_display(messages, tool_msg_idx) else {
        return body;
    };
    let Some(prefix) = crate::tools::summarize_tool_call(&name, &args) else {
        return body;
    };
    let t = prefix.trim();
    if t.is_empty() {
        return body;
    }
    // JSON 已以 human_summary 开头且与 summarize 重复时不再加前缀
    if raw.trim().starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(raw.trim())
        && let Some(h) = v.get("human_summary").and_then(|x| x.as_str())
    {
        let hs = h.trim();
        if hs == t || hs.contains(t) || (t.len() > 5 && t.contains(hs)) {
            return body;
        }
    }
    if body.is_empty() {
        return format!("【描述与总结】\n{prefix}");
    }
    format!("【描述与总结】\n{prefix}\n\n{body}")
}

// --- 助手正文：剥重复「模型：」标签 → 规划可读化 → LaTeX（与 TUI / CLI `terminal_render_agent_markdown` 共用）---

/// 是否在助手气泡 / CLI 终端中展示「分阶段规划轮」产出的 `agent_reply_plan` 正文（可读化后的列表等）。
/// `false` 时：可解析为 v1 规划的助手消息在**展示层**置空（`Message.content` 仍保留 JSON 供后续解析）；右栏「队列」与 `staged_plan_notice` 不受影响。
pub(crate) const SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT: bool = false;

/// 是否在聊天区展示 **`agent_turn` 分步注入的 `user` 消息**（`【分步执行 i/n】…\n- id: …\n- 描述: …`）。
/// `false` 时**整段**在展示层置空（与 `run_staged_plan_then_execute_steps` 注入格式一致）；`Message.content`、导出与 `log`（含 `debug!`）仍为全文。
pub(crate) const SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT: bool = false;

/// 与 `run_staged_plan_then_execute_steps` 注入的 user 正文同形（宽松匹配，避免误伤普通用户输入）。
fn is_staged_step_injection_user_content(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with("【分步执行") && t.contains("\n- id:") && t.contains("\n- 描述:")
}

/// `user` 气泡 / CLI 用户侧展示。
pub(crate) fn user_message_for_chat_display(raw: &str) -> String {
    if !SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT && is_staged_step_injection_user_content(raw) {
        return String::new();
    }
    latex_math_to_unicode(raw)
}

/// TUI 已单独画一行「模型:」；正文里常见 `模型：…`、`## 模型：`、`**模型：**` 等重复标签，用正则循环剥掉。
static ASSISTANT_LEADING_ROLE_ECHO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        ^[\s\u{feff}\u{3000}]*
        (?:
            (?: \#+ | > ) \s*
            (模型|助手|Assistant|Model)
            \s* [：:]
          | \*{1,2} \s* (模型|助手|Assistant|Model) \s* [：:] \s* \*{1,2}
          | _{1,2} \s* (模型|助手|Assistant|Model) \s* [：:] \s* _{1,2}
          | 【 \s* 模型 \s* 】 \s* [：:]
          | (模型|助手|Assistant|Model) \s* [：:]
        )
        \s*",
    )
    .expect("ASSISTANT_LEADING_ROLE_ECHO")
});

/// 整行只有「角色称呼」时（含 `# 模型：`、`**模型：**` 等），与 TUI 顶栏「模型:」重复，应剥掉。
static STANDALONE_ROLE_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x) ^ \s*
        (?: \#+ \s* )?
        (?: > \s* )?
        (?: \*{1,2} | _{1,2} )? \s*
        (?: 【 \s* 模型 \s* 】 \s* [：:] | (模型|助手|Assistant|Model) \s* [：:] )
        \s*
        (?: \*{1,2} | _{1,2} )? \s*
        $",
    )
    .expect("STANDALONE_ROLE_LINE")
});

fn is_standalone_role_echo_line(t: &str) -> bool {
    let t = t.trim().trim_matches('\u{3000}');
    if t.is_empty() {
        return false;
    }
    matches!(
        t,
        "模型"
            | "模型："
            | "模型:"
            | "Assistant"
            | "Assistant："
            | "Assistant:"
            | "助手"
            | "助手："
            | "助手:"
            | "Model"
            | "Model："
            | "Model:"
    ) || STANDALONE_ROLE_LINE.is_match(t)
}

fn strip_leading_blank_and_role_lines(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim().trim_matches('\u{3000}');
        if t.is_empty() || is_standalone_role_echo_line(t) {
            i += 1;
            continue;
        }
        break;
    }
    lines[i..].join("\n")
}

/// 剥掉正文前导的「模型/助手」重复标签（与 TUI 顶栏分工）。
pub(crate) fn strip_assistant_echo_label(content: &str) -> String {
    let mut s = content
        .trim_start()
        .trim_start_matches('\u{feff}')
        .to_string();
    for _ in 0..32 {
        let before = s.clone();
        for _ in 0..12 {
            let trimmed = s.trim_start().trim_start_matches('\u{feff}');
            let next = ASSISTANT_LEADING_ROLE_ECHO.replace(trimmed, "");
            let next = next.trim_start().trim_start_matches('\u{feff}').to_string();
            if next == s {
                break;
            }
            s = next;
        }
        s = strip_leading_blank_and_role_lines(&s);
        if s == before {
            break;
        }
    }
    s
}

/// 流式阶段检测到「正在输出 agent_reply_plan」时，聊天区不刷 JSON 片段，展示**本占位 + 围栏前说明**（占位置顶）；**收齐后**走 [`assistant_markdown_source_for_display`] 再剥 JSON。
const STAGED_PLAN_STREAMING_PLACEHOLDER: &str = "正在生成分阶段规划…";
const STAGED_PLAN_STREAMING_PLACEHOLDER_BASE: &str = "正在生成分阶段规划";

fn triple_backtick_fence_count(s: &str) -> usize {
    s.match_indices("```").count()
}

/// 首段代码围栏（`parts[1]`）视为「JSON 规划流」：`json` 语言行后为空或 `{` 开头，或无语言行且以内联 `{` 开头。
fn first_fence_inner_looks_like_json_object(s: &str) -> bool {
    let mut it = s.split("```");
    let _ = it.next();
    let Some(inner) = it.next() else {
        return false;
    };
    let rest = inner.trim_start();
    let first_line = rest.lines().next().unwrap_or("").trim();
    if first_line.eq_ignore_ascii_case("json") {
        let body: String = rest.lines().skip(1).collect::<Vec<_>>().join("\n");
        let b = body.trim();
        return b.is_empty() || b.starts_with('{');
    }
    rest.trim().starts_with('{')
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

/// 流式未结束时：若判定为 agent_reply_plan 相关输出，则不在 UI 上逐 delta 渲染 JSON。
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
    if parse_agent_reply_plan_v1(stripped).is_ok() {
        return false;
    }
    serde_json::from_str::<serde_json::Value>(t).is_err()
        && looks_like_incomplete_agent_reply_plan_whole_json(t)
}

fn is_staged_plan_placeholder_like_line(line: &str) -> bool {
    let t = line.trim();
    if t.is_empty() {
        return false;
    }
    let t = t.trim_end_matches(|c: char| {
        matches!(c, '…' | '.' | '。' | '!' | '！' | '?' | '？' | ':' | '：')
    });
    let t = t.trim();
    t == STAGED_PLAN_STREAMING_PLACEHOLDER_BASE
        || t.starts_with(STAGED_PLAN_STREAMING_PLACEHOLDER_BASE)
}

fn drop_leading_placeholder_like_prose_line(prose: &str) -> String {
    let mut lines = prose.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };
    if !is_staged_plan_placeholder_like_line(first) {
        return prose.to_string();
    }
    lines.collect::<Vec<_>>().join("\n").trim().to_string()
}

fn staged_plan_streaming_chat_body(stripped: &str) -> String {
    let raw = prose_before_first_fence(stripped);
    // 与收齐后 `staged_plan_hidden_chat_prose_only` 一致：DSML、相邻重复行、列表并句，避免流式阶段出现双行复读而收齐后变单段等不一致。
    let prose_t = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw);
    let prose_t = prose_t.trim();
    if prose_t.is_empty() {
        STAGED_PLAN_STREAMING_PLACEHOLDER.to_string()
    } else {
        // TUI 默认「跟随尾部」滚动；将占位放在末尾可避免长开场白把「正在生成…」挤出可视区。
        // 若模型开场白本身就是同义占位（如“正在生成分阶段规划...”），去重后仅保留一处。
        let prose_wo_dup = drop_leading_placeholder_like_prose_line(prose_t);
        if prose_wo_dup.is_empty() {
            STAGED_PLAN_STREAMING_PLACEHOLDER.to_string()
        } else {
            format!("{prose_wo_dup}\n\n{STAGED_PLAN_STREAMING_PLACEHOLDER}")
        }
    }
}

/// 主聊天区隐藏 v1 规划列表时，仍展示首个 \`\`\` 围栏**之前**的自然语言（与 `format_agent_reply_plan_for_display` 的 goal 段一致），
/// 避免 JSON 一收齐展示层整段置空、首句在「打印分阶段规划/队列更新」瞬间消失。
fn staged_plan_hidden_chat_prose_only(original: &str) -> String {
    let raw_goal = prose_before_first_fence(original);
    let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
    let goal_t = goal.trim();
    let merged = match parse_agent_reply_plan_v1(original) {
        Ok(plan) => augment_agent_reply_plan_goal_for_display(goal_t, &plan),
        Err(_) => goal_t.to_string(),
    };
    let merged = merged.trim();
    if merged.is_empty() {
        String::new()
    } else {
        latex_math_to_unicode(merged)
    }
}

/// 剥标签后的助手正文：可读化规划、去围栏、LaTeX（**非流式**完整处理）。
fn assistant_markdown_from_stripped(stripped: &str) -> String {
    if SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT {
        let display =
            format_agent_reply_plan_for_display(stripped).unwrap_or_else(|| stripped.to_string());
        return latex_math_to_unicode(&display);
    }
    if parse_agent_reply_plan_v1(stripped).is_ok() {
        return staged_plan_hidden_chat_prose_only(stripped);
    }
    let without_fences = strip_agent_reply_plan_fence_blocks_for_display(stripped);
    let trimmed = without_fences.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if parse_agent_reply_plan_v1(trimmed).is_ok() {
        return staged_plan_hidden_chat_prose_only(stripped);
    }
    let display = format_agent_reply_plan_for_display(&without_fences).unwrap_or(without_fences);
    latex_math_to_unicode(&display)
}

/// 助手气泡 / CLI ANSI / 导出共用：剥标签 → `agent_reply_plan` 可读化 → LaTeX。
/// `SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT` 为 `false` 时：可解析为 v1 规划 → 不展示列表/JSON，但**保留**围栏前自然语言概括；纯 JSON 无前置说明时仍为空。
/// 若仅围栏内为规划 JSON（含解析失败但形状明显的块），从展示串中移除围栏，**不**把原始 JSON 打到终端/气泡；`Message.content` 与日志不变。
pub(crate) fn assistant_markdown_source_for_display(raw: &str) -> String {
    let stripped = strip_assistant_echo_label(raw);
    let stripped = preprocess_unfenced_assistant_prose_dedup(&stripped);
    assistant_markdown_from_stripped(&stripped)
}

/// TUI 流式：仅对**最后一条助手**且仍处生成中时调用；`agent_reply_plan` 相关输出缓冲为占位，收齐后由普通路径一次性剥 JSON 再展示。
pub(crate) fn assistant_markdown_source_for_display_streaming_last(raw: &str) -> String {
    let stripped = strip_assistant_echo_label(raw);
    if should_buffer_agent_reply_plan_stream(&stripped) {
        return latex_math_to_unicode(&staged_plan_streaming_chat_body(&stripped));
    }
    let stripped = preprocess_unfenced_assistant_prose_dedup(&stripped);
    assistant_markdown_from_stripped(&stripped)
}

/// 在打出首个 \`\`\` 之前，`should_buffer` 为 false，正文不经规划专用清洗；此处对**无围栏、非整段 JSON** 的助手气泡做与围栏前一致的复读折叠。
fn preprocess_unfenced_assistant_prose_dedup(stripped: &str) -> String {
    if stripped.contains("```") {
        return stripped.to_string();
    }
    let t = stripped.trim_start();
    if t.starts_with('{') {
        return stripped.to_string();
    }
    crate::text_sanitize::dedupe_plain_assistant_preamble(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, Message, ToolCall};

    #[test]
    fn tool_json_human_summary_then_result_block() {
        let raw = r#"{"human_summary":"编译成功","ok":true}"#;
        let out = tool_content_for_display_impl(raw, true);
        assert!(out.starts_with("编译成功"));
        assert!(out.contains("【执行结果】"));
        assert!(out.contains("\"ok\": true"));
    }

    #[test]
    fn tool_json_hides_pretty_json_in_chat_mode() {
        let raw = r#"{"human_summary":"编译成功","ok":true}"#;
        let out = tool_content_for_display_impl(raw, false);
        assert_eq!(out, "编译成功");
        assert!(!out.contains("【执行结果】"));
        assert!(!out.contains("\"ok\""));
    }

    #[test]
    fn tool_non_json_is_passthrough() {
        let raw = "plain tool output";
        assert_eq!(
            tool_content_for_display_impl(raw, true),
            "plain tool output"
        );
        assert_eq!(tool_content_for_display_impl(raw, false), "");
    }

    #[test]
    fn tool_plain_run_command_structured() {
        let raw = "退出码：0\n标准输出：\nhello\n";
        let out = tool_content_for_display_impl(raw, true);
        assert!(out.contains("【执行结果】"));
        assert!(out.contains("状态："));
        assert!(out.contains("成功"));
        assert!(out.contains("标准输出："));
        assert!(out.contains("hello"));
        assert!(!out.lines().next().unwrap_or("").starts_with("退出码："));
    }

    #[test]
    fn tool_plain_run_command_structured_hides_stdout_in_chat_mode() {
        let raw = "退出码：0\n标准输出：\nhello\n";
        let out = tool_content_for_display_impl(raw, false);
        assert!(out.is_empty());
    }

    #[test]
    fn tool_for_message_prepends_summary_from_assistant_tool_calls() {
        let messages = vec![
            Message::user_only("hi"),
            Message {
                role: "assistant".into(),
                content: Some("I'll run ls".into()),
                tool_calls: Some(vec![ToolCall {
                    id: "c1".into(),
                    typ: "function".into(),
                    function: FunctionCall {
                        name: "run_command".into(),
                        arguments: r#"{"command":"ls","args":[]}"#.into(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".into(),
                content: Some("退出码：0\n(无输出)".into()),
                tool_calls: None,
                name: None,
                tool_call_id: Some("c1".into()),
            },
        ];
        let raw = messages[2].content.as_deref().unwrap();
        let out = tool_content_for_display_for_message(raw, &messages, 2);
        assert_eq!(out, "【描述与总结】\n执行命令：ls");
        assert!(!out.contains("【执行结果】"));
    }

    #[test]
    fn assistant_strips_leading_model_colon() {
        let raw = "模型：\n\n正文";
        let out = assistant_markdown_source_for_display(raw);
        assert!(out.contains("正文"));
        assert!(!out.contains("模型："));
    }

    #[test]
    fn assistant_pipeline_matches_strip_then_plan_latex() {
        let raw = "模型：\nhello";
        let stripped = strip_assistant_echo_label(raw);
        let mid = format_agent_reply_plan_for_display(&stripped).unwrap_or(stripped);
        let expected = latex_math_to_unicode(&mid);
        assert_eq!(assistant_markdown_source_for_display(raw), expected);
    }

    #[test]
    fn assistant_hides_staged_plan_v1_when_show_flag_false() {
        let raw =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do"}]}"#;
        assert_eq!(assistant_markdown_source_for_display(raw), "");
    }

    #[test]
    fn assistant_hides_plan_json_in_fence_keeps_prose_when_show_flag_false() {
        let raw = format!(
            "说明文字\n```json\n{}\n```\n",
            r#"{"type":"agent_reply_plan","version":1,"steps":[]}"#
        );
        let out = assistant_markdown_source_for_display(&raw);
        assert!(out.contains("说明"));
        assert!(!out.contains("agent_reply_plan"));
    }

    #[test]
    fn assistant_valid_fenced_plan_keeps_prose_prefix_when_show_flag_false() {
        let raw = format!(
            "下面拆解任务。\n```json\n{}\n```\n",
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x"}]}"#
        );
        let out = assistant_markdown_source_for_display(&raw);
        assert!(out.contains("下面拆解"));
        assert!(!out.contains("agent_reply_plan"));
        assert!(!out.contains("```"));
    }

    #[test]
    fn assistant_fenced_plan_dedupes_identical_prose_lines_before_fence() {
        let line = "我将帮您编写一个简单的 C++ Hello World 程序，让我先规划任务步骤：";
        let raw = format!(
            "{line}\n{line}\n```json\n{}\n```\n",
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"创建源文件"}]}"#
        );
        let out = assistant_markdown_source_for_display(&raw);
        assert_eq!(out.matches(line).count(), 1, "{}", out);
    }

    #[test]
    fn assistant_streaming_last_dedupes_duplicate_prose_before_partial_fence() {
        let line = "我将帮您编写一个简单的 C++ Hello World 程序，让我先规划任务步骤：";
        let raw = format!("{line}\n{line}\n```json\n{{\"type\":\"agent_reply_plan\"");
        let out = assistant_markdown_source_for_display_streaming_last(&raw);
        assert_eq!(out.matches(line).count(), 1, "{}", out);
        assert!(out.contains("正在生成分阶段规划"));
    }

    #[test]
    fn assistant_streaming_last_buffers_partial_fenced_plan_json() {
        let raw = "下面拆解如下。\n```json\n{\"type\":\"agent_reply_plan\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.contains("下面拆解"));
        assert!(out.contains("正在生成分阶段规划"));
        assert!(!out.contains("\"steps\""));
    }

    #[test]
    fn assistant_streaming_last_placeholder_after_fence_prose() {
        let raw = "这个任务可以分成以下步骤\n```json\n{\"type\":\"agent_reply_plan\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        let ph = out.find("正在生成分阶段规划").expect("placeholder");
        let prose = out
            .find("这个任务可以分成以下步骤")
            .expect("fence-before prose");
        assert!(
            ph > prose,
            "占位置尾，避免跟随尾部时被开场白挤出可视区：out={out:?}"
        );
    }

    #[test]
    fn assistant_streaming_last_dedupes_placeholder_like_opening_line() {
        let raw = "正在生成分阶段规划...\n```json\n{\"type\":\"agent_reply_plan\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert_eq!(
            out.matches("正在生成分阶段规划").count(),
            1,
            "开场白与占位同义时应去重：{out:?}"
        );
    }

    #[test]
    fn assistant_streaming_last_plain_text_still_incremental() {
        let raw = "先写一句说明，再考虑格式。";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.contains("先写一句"));
        assert!(!out.contains("正在生成分阶段规划"));
    }

    /// 尚未输出 ``` 时 `should_buffer` 为 false；此前不经规划专用 naturalize，需在预处理中折叠围栏前同句复读。
    #[test]
    fn assistant_streaming_last_dedupes_duplicate_lines_before_any_fence() {
        let line = "我将帮您编写 C++ Hello World，让我先规划任务步骤：";
        let raw = format!("{line}\n{line}");
        let out = assistant_markdown_source_for_display_streaming_last(&raw);
        assert_eq!(out.matches(line).count(), 1, "{}", out);
        assert!(!out.contains("正在生成分阶段规划"));
    }

    #[test]
    fn assistant_streaming_last_whole_json_incomplete_uses_placeholder() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x""#;
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.contains("正在生成分阶段规划"));
    }

    #[test]
    fn user_hides_staged_step_injection_when_show_flag_false() {
        let raw = format!(
            "【分步执行 1/2】{}\n- id: s1\n- 描述: 读文件",
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE
        );
        assert_eq!(user_message_for_chat_display(&raw), "");
    }
}
