//! 对话消息在 UI/终端上的展示用正文（与 `Message.content` 存储形态解耦）。

use regex::Regex;
use std::sync::LazyLock;

use crate::latex_unicode::latex_math_to_unicode;
use crate::message_display_parts::tool_display_from_normalized_envelope;
use crabmate_agent::plan_artifact::{
    format_agent_reply_plan_for_display, strip_agent_reply_plan_fence_blocks_for_display,
};
use crabmate_tools::tool_result::{ToolResult, normalize_tool_message_content};
use crabmate_types::Message;

/// 工具结果中「原始输出」块的 Markdown 小标题（与 Web `ChatPanel`、CLI 完整回显一致）。
pub use crate::message_display_parts::TOOL_OUTPUT_SECTION_HEADLINE;

/// 聊天区（Web 工具卡等）是否展示 **`### 执行输出`** 整块（状态行、stdout/stderr、完整 JSON、纯文本正文等）。
/// `false` 时仅展示 `summarize_tool_call` / JSON `human_summary` 等摘要；**不打印**「`### 执行输出`」及其下任何内容；`Message.content` 与 tracing 仍为全文。
pub const SHOW_TOOL_RAW_OUTPUT_IN_CHAT: bool = false;

/// `role: tool` 的展示：与 Web `ChatPanel` 的 `buildToolOutputCardText` 对齐。
/// [`SHOW_TOOL_RAW_OUTPUT_IN_CHAT`] 为 `false` 时仅 JSON `human_summary` 等摘要，**无**「`### 执行输出`」；
/// 为 `true` 时：先 `human_summary`，再 **`### 执行输出`**（状态 + stdout/stderr 等）；纯文本 `run_command` 风格则结构化展示。
///
/// 受 [`SHOW_TOOL_RAW_OUTPUT_IN_CHAT`] 控制；CLI 无 SSE 回显请用 [`tool_content_for_display_full`]。
pub fn tool_content_for_display(raw: &str) -> String {
    tool_content_for_display_impl(raw, SHOW_TOOL_RAW_OUTPUT_IN_CHAT)
}

/// 终端 CLI 等需与「聊天区省略策略」独立时：始终包含完整工具输出（与日志/对话历史一致）。
pub fn tool_content_for_display_full(raw: &str) -> String {
    tool_content_for_display_impl(raw, true)
}

pub fn tool_content_for_display_impl(raw: &str, include_raw: bool) -> String {
    let t = raw.trim();
    if t.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(t)
    {
        if let Some(env) = normalize_tool_message_content(t) {
            return tool_display_from_normalized_envelope(&v, t, &env, include_raw);
        }
        return tool_display_from_json_value(&v, t, include_raw);
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

fn tool_display_from_json_value(v: &serde_json::Value, t: &str, include_raw: bool) -> String {
    if include_raw {
        if let Some(h) = v.get("human_summary").and_then(|x| x.as_str()) {
            let pretty = serde_json::to_string_pretty(v).unwrap_or_else(|_| t.to_string());
            return format!("{h}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}");
        }
        return serde_json::to_string_pretty(v).unwrap_or_else(|_| t.to_string());
    }
    if let Some(h) = v.get("human_summary").and_then(|x| x.as_str()) {
        let hs = h.trim();
        if hs.is_empty() {
            return String::new();
        }
        return hs.to_string();
    }
    String::new()
}

fn should_format_as_structured_plain_tool(raw: &str) -> bool {
    for line in raw.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if line.starts_with("命令：") {
            continue;
        }
        if line.starts_with("退出码：") {
            return true;
        }
        if line.contains("(exit=") && line.contains(')') {
            return true;
        }
        break;
    }
    raw.contains("标准输出：\n") || raw.contains("标准错误：\n")
}

fn strip_first_tool_status_line(raw: &str) -> String {
    let mut lines: Vec<&str> = raw.lines().collect();
    while let Some(first) = lines.first() {
        let t = first.trim();
        if t.starts_with("命令：")
            || t.starts_with("退出码：")
            || (t.contains("(exit=") && t.contains(')'))
        {
            lines.remove(0);
            continue;
        }
        break;
    }
    lines.join("\n").trim().to_string()
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

    format!("{TOOL_OUTPUT_SECTION_HEADLINE}\n{status_line}\n{result_body}")
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

/// **导出 / 运维 CLI**：信封走 [`tool_content_for_display`]（摘要或截断原文）；聊天省略策略下
/// 正文为空时再回退 `summarize_tool_call`。不生成 Web 像素级工具卡（W2b：`crabmate-tool-card` 已迁 Client）。
pub fn tool_content_for_display_for_message(
    raw: &str,
    messages: &[Message],
    tool_msg_idx: usize,
) -> String {
    let body = tool_content_for_display(raw);
    if !body.trim().is_empty() {
        return body;
    }
    let Some((name, args)) = find_tool_call_for_display(messages, tool_msg_idx) else {
        return body;
    };
    crabmate_tools::tools::summarize_tool_call(&name, &args)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(body)
}

// --- 助手正文：剥重复「模型：」标签 → 规划可读化 → LaTeX（Web / SSE 展示管线）---

/// `user` 气泡 / CLI 用户侧展示。
pub fn user_message_for_chat_display(raw: &str) -> String {
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
pub fn strip_assistant_echo_label(content: &str) -> String {
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

/// 剥标签后的助手正文：可读化规划、去围栏、LaTeX（**非流式**完整处理）。
fn assistant_markdown_from_stripped(stripped: &str) -> String {
    if let Some(display) = format_agent_reply_plan_for_display(stripped) {
        return latex_math_to_unicode(&display);
    }
    let without_fences = strip_agent_reply_plan_fence_blocks_for_display(stripped);
    let trimmed = without_fences.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let display = format_agent_reply_plan_for_display(&without_fences).unwrap_or(without_fences);
    latex_math_to_unicode(&display)
}

/// 助手气泡 / CLI ANSI / 导出共用：剥标签 → `agent_reply_plan` 可读化 → LaTeX。
/// 若仅围栏内为规划 JSON（含解析失败但形状明显的块），从展示串中移除围栏，**不**把原始 JSON 打到终端/气泡；`Message.content` 与日志不变。
pub fn assistant_markdown_source_for_display(raw: &str) -> String {
    let stripped = strip_assistant_echo_label(raw);
    let stripped = preprocess_unfenced_assistant_prose_dedup(&stripped);
    assistant_markdown_from_stripped(&stripped)
}

/// TUI 流式：拼接思维链与正文（与 `llm::api` 累加顺序一致）。收齐后由 [`assistant_markdown_source_for_display`] 剥规划 JSON。
pub fn assistant_streaming_plain_concat(m: &Message) -> String {
    let mut s = String::new();
    if let Some(r) = m.reasoning_content.as_deref() {
        s.push_str(r);
    }
    if let Some(c) = crabmate_types::message_content_as_str(&m.content) {
        s.push_str(c);
    }
    s
}

/// `deepseek-reasoner` 等：拼接「思考过程」与正文为 Markdown 源，再走 [`assistant_markdown_source_for_display`]。
pub fn assistant_markdown_source_for_message(m: &Message) -> String {
    let raw = assistant_raw_markdown_body_from_parts(
        m.reasoning_content.as_deref().unwrap_or(""),
        crabmate_types::message_content_as_str(&m.content).unwrap_or(""),
    );
    assistant_markdown_source_for_display(&raw)
}

/// 展示用：有思维链时加小标题与分隔线，再拼接最终回答。
pub fn assistant_raw_markdown_body_from_parts(reasoning: &str, content: &str) -> String {
    let r = reasoning.trim();
    let c = content.trim();
    match (r.is_empty(), c.is_empty()) {
        (false, false) => format!("#\u{0023}# 思考过程\n\n{r}\n\n---\n\n{c}"),
        (false, true) => format!("#\u{0023}# 思考过程\n\n{r}"),
        (true, false) => c.to_string(),
        (true, true) => String::new(),
    }
}

/// 与 [`assistant_raw_markdown_body_from_parts`] 相同，从已组装的 [`Message`] 读取字段。
pub fn assistant_raw_markdown_body_for_message(m: &Message) -> String {
    assistant_raw_markdown_body_from_parts(
        m.reasoning_content.as_deref().unwrap_or(""),
        crabmate_types::message_content_as_str(&m.content).unwrap_or(""),
    )
}

/// 对助手正文做围栏前复读折叠：无围栏时整段处理；**有围栏时仍只处理首个 ` ``` ` 之前**。
/// 流式规划缓冲逻辑在前端 `message_format/display/plan_fence.rs`。
fn preprocess_unfenced_assistant_prose_dedup(stripped: &str) -> String {
    let t = stripped.trim_start();
    if t.starts_with('{') {
        return stripped.to_string();
    }
    if let Some(idx) = stripped.find("```") {
        let (pre, from_fence) = stripped.split_at(idx);
        let pre_deduped = crabmate_agent::text_sanitize::dedupe_plain_assistant_preamble(pre);
        format!("{pre_deduped}{from_fence}")
    } else {
        crabmate_agent::text_sanitize::dedupe_plain_assistant_preamble(stripped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crabmate_types::{FunctionCall, Message, ToolCall};

    #[test]
    fn tool_json_human_summary_then_result_block() {
        let raw = r#"{"human_summary":"编译成功","ok":true}"#;
        let out = tool_content_for_display_impl(raw, true);
        assert!(out.starts_with("编译成功"));
        assert!(out.contains(TOOL_OUTPUT_SECTION_HEADLINE));
        assert!(out.contains("\"ok\": true"));
    }

    #[test]
    fn tool_json_hides_pretty_json_in_chat_mode() {
        let raw = r#"{"human_summary":"编译成功","ok":true}"#;
        let out = tool_content_for_display_impl(raw, false);
        assert_eq!(out, "编译成功");
        assert!(!out.contains(TOOL_OUTPUT_SECTION_HEADLINE));
        assert!(!out.contains("\"ok\""));
    }

    #[test]
    fn tool_crabmate_envelope_uses_summary_field() {
        let raw = r#"{"crabmate_tool":{"v":1,"name":"read_file","summary":"读文件：a.rs","ok":true,"output":"content"}}"#;
        assert_eq!(tool_content_for_display_impl(raw, false), "读文件：a.rs");
        let full = tool_content_for_display_impl(raw, true);
        assert!(full.starts_with("读文件：a.rs"));
        assert!(full.contains(TOOL_OUTPUT_SECTION_HEADLINE));
        assert!(full.contains("crabmate_tool"));
    }

    #[test]
    fn tool_crabmate_truncated_shows_note_beside_summary() {
        let raw = r#"{"crabmate_tool":{"v":1,"name":"run_command","summary":"grep","ok":true,"output":"x","output_truncated":true,"output_original_chars":9999,"output_kept_head_chars":40,"output_kept_tail_chars":40}}"#;
        let chat = tool_content_for_display_impl(raw, false);
        assert!(chat.contains("grep"));
        assert!(chat.contains("输出已压缩入上下文"));
        assert!(chat.contains("9999"));
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
        assert!(out.contains(TOOL_OUTPUT_SECTION_HEADLINE));
        assert!(out.contains("状态："));
        assert!(out.contains("成功"));
        assert!(out.contains("标准输出："));
        assert!(out.contains("hello"));
        assert!(!out.lines().next().unwrap_or("").starts_with("退出码："));
    }

    #[test]
    fn tool_plain_run_command_structured_with_command_line() {
        let raw = "命令：echo hi\n退出码：0\n标准输出：\nhi\n";
        let out = tool_content_for_display_impl(raw, true);
        assert!(out.contains(TOOL_OUTPUT_SECTION_HEADLINE));
        assert!(out.contains("成功"));
        assert!(out.contains("hi"));
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
                reasoning_content: None,
                reasoning_details: None,
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
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("c1".into()),
            },
        ];
        let raw = crabmate_types::message_content_as_str(&messages[2].content).unwrap();
        let out = tool_content_for_display_for_message(raw, &messages, 2);
        assert_eq!(out, "ls");
        assert!(!out.contains(TOOL_OUTPUT_SECTION_HEADLINE));
    }

    #[test]
    fn tool_for_message_uses_envelope_summary_not_pixel_tool_card() {
        let envelope = r#"{"crabmate_tool":{"v":1,"name":"read_file","summary":"读：a.rs","ok":true,"output":"content"}}"#;
        let messages = vec![
            Message::user_only("hi"),
            Message {
                role: "assistant".into(),
                content: Some("read".into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "c1".into(),
                    typ: "function".into(),
                    function: FunctionCall {
                        name: "read_file".into(),
                        arguments: r#"{"path":"a.rs"}"#.into(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            Message {
                role: "tool".into(),
                content: Some(envelope.into()),
                reasoning_content: None,
                reasoning_details: None,
                tool_calls: None,
                name: None,
                tool_call_id: Some("c1".into()),
            },
        ];
        let out = tool_content_for_display_for_message(envelope, &messages, 2);
        assert_eq!(out, "读：a.rs");
        assert!(!out.contains("crabmate_tool"));
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
    fn assistant_formats_agent_reply_plan_v1_for_display() {
        let raw =
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"do"}]}"#;
        let out = assistant_markdown_source_for_display(raw);
        assert!(out.contains("1. `a`: do"));
        assert!(!out.contains("agent_reply_plan"));
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
        assert!(out.contains("下面拆解任务"));
        assert!(!out.contains("agent_reply_plan"));
        assert!(!out.contains("```"));
    }

    #[test]
    fn assistant_valid_plan_keeps_preamble_when_present() {
        let raw = format!(
            "我将帮您编写一个简单的C++ Hello World程序，并完成编译执行。以下是任务拆解：\n```json\n{}\n```\n",
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x"}]}"#
        );
        let out = assistant_markdown_source_for_display(&raw);
        assert!(
            out.contains("我将帮您编写一个简单的C++ Hello World程序"),
            "{out}"
        );
        assert!(!out.contains("已生成分阶段规划"), "{out}");
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
}
