//! 对话消息在 UI/终端上的展示用正文（与 `Message.content` 存储形态解耦）。
#![allow(dead_code)]

use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::LazyLock;

use crate::agent::plan_artifact::{
    augment_agent_reply_plan_goal_for_display, fenced_body_after_optional_jsonish_lang_label,
    format_agent_reply_plan_for_display, parse_agent_reply_plan_v1, prose_before_first_fence,
    strip_agent_reply_plan_fence_blocks_for_display,
};
use crate::runtime::latex_unicode::latex_math_to_unicode;
use crate::tool_result::{ToolResult, normalize_tool_message_content};
use crate::types::Message;

/// 工具结果中「原始输出」块的 Markdown 小标题（与 Web `ChatPanel`、CLI 完整回显一致）。
pub(crate) const TOOL_OUTPUT_SECTION_HEADLINE: &str = "### 执行输出";

/// 聊天区（Web 工具卡等）是否展示 **`### 执行输出`** 整块（状态行、stdout/stderr、完整 JSON、纯文本正文等）。
/// `false` 时仅展示 `summarize_tool_call` / JSON `human_summary` 等摘要；**不打印**「`### 执行输出`」及其下任何内容；`Message.content` 与 tracing 仍为全文。
pub(crate) const SHOW_TOOL_RAW_OUTPUT_IN_CHAT: bool = false;

/// `role: tool` 的展示：与 Web `ChatPanel` 的 `buildToolOutputCardText` 对齐。
/// [`SHOW_TOOL_RAW_OUTPUT_IN_CHAT`] 为 `false` 时仅 JSON `human_summary` 等摘要，**无**「`### 执行输出`」；
/// 为 `true` 时：先 `human_summary`，再 **`### 执行输出`**（状态 + stdout/stderr 等）；纯文本 `run_command` 风格则结构化展示。
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
        if let Some(env) = normalize_tool_message_content(t) {
            let summary = env.summary.trim();
            let trunc_note = if env.output_truncated {
                let orig = env
                    .output_original_chars
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let head = env
                    .output_kept_head_chars
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let tail = env
                    .output_kept_tail_chars
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "?".to_string());
                Some(format!(
                    "（输出已压缩入上下文：原文约 {orig} 字符，保留首尾约 {head}+{tail} 字符；见 `output` 内采样与说明。）"
                ))
            } else {
                None
            };
            let struct_note = {
                let mut parts: Vec<String> = Vec::new();
                if env.execution_mode.as_deref() == Some("parallel_readonly_batch")
                    && let Some(ref bid) = env.parallel_batch_id
                    && !bid.is_empty()
                {
                    parts.push(format!("并行只读批次 `{bid}`"));
                }
                if env.retryable == Some(true) {
                    parts.push("失败可能可重试（启发式 `retryable`）".to_string());
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(format!("（{}）", parts.join("；")))
                }
            };
            let mut note_lines: Vec<String> = Vec::new();
            if let Some(ref n) = trunc_note
                && !n.is_empty()
            {
                note_lines.push(n.clone());
            }
            if let Some(ref n) = struct_note
                && !n.is_empty()
            {
                note_lines.push(n.clone());
            }
            let combined_note = if note_lines.is_empty() {
                None
            } else {
                Some(note_lines.join("\n"))
            };
            if include_raw {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| t.to_string());
                if summary.is_empty() {
                    return match combined_note {
                        Some(ref note) if !note.is_empty() => {
                            format!("{note}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}")
                        }
                        _ => format!("{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}"),
                    };
                }
                return match combined_note {
                    Some(ref note) if !note.is_empty() => {
                        format!("{summary}\n{note}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}")
                    }
                    _ => format!("{summary}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}"),
                };
            }
            if summary.is_empty() {
                return combined_note.unwrap_or_default();
            }
            return match combined_note {
                Some(note) if !note.is_empty() => format!("{summary}\n{note}"),
                _ => summary.to_string(),
            };
        }
        if include_raw {
            if let Some(h) = v.get("human_summary").and_then(|x| x.as_str()) {
                let pretty = serde_json::to_string_pretty(&v).unwrap_or_else(|_| t.to_string());
                return format!("{h}\n\n{TOOL_OUTPUT_SECTION_HEADLINE}\n{pretty}");
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

/// TUI 行缓存指纹：同一条 `tool` 消息在「assistant 已带上 tool_calls」前后，展示可能多出一节 `summarize_tool_call` 摘要。
pub(crate) fn tool_display_context_fingerprint(messages: &[Message], tool_msg_idx: usize) -> u64 {
    let mut h = DefaultHasher::new();
    if let Some((name, args)) = find_tool_call_for_display(messages, tool_msg_idx) {
        name.hash(&mut h);
        args.hash(&mut h);
    }
    h.finish()
}

/// **TUI / 导出**：在 [`tool_content_for_display`] 之上，为 `role: tool` 追加与 Web 一致的 `summarize_tool_call` 摘要
///（依赖历史中**对条** assistant 的 `tool_calls`）。
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
        return prefix.to_string();
    }
    format!("{prefix}\n\n{body}")
}

// --- 助手正文：剥重复「模型：」标签 → 规划可读化 → LaTeX（与 TUI / CLI `terminal_render_agent_markdown` 共用）---

/// 是否在助手气泡 / CLI 终端中展示「分阶段规划轮」产出的 `agent_reply_plan` 正文（可读化后的列表等）。
/// `false` 时：可解析为 v1 规划的助手消息在**展示层**置空（`Message.content` 仍保留 JSON 供后续解析）；右栏「队列」与 `staged_plan_notice` 不受影响。
pub(crate) const SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT: bool = false;

/// 是否在聊天区展示 **`agent_turn` 分步注入的 `user` 消息**（`### 分步 i/n`…`\n- id:`…`\n- 描述:`…；历史会话可能仍为 `【分步执行` 前缀）。
/// `false` 时**整段**在展示层置空（与 `run_staged_plan_then_execute_steps` 注入格式一致）；`Message.content`、导出与 `log`（含 `debug!`）仍为全文。
pub(crate) const SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT: bool = false;

/// 与 `run_staged_plan_then_execute_steps` 注入的 user 正文同形（宽松匹配，避免误伤普通用户输入）。
fn is_staged_step_injection_user_content(s: &str) -> bool {
    let t = s.trim_start();
    if !(t.contains("\n- id:") && t.contains("\n- 描述:")) {
        return false;
    }
    t.starts_with("### 分步 ") || t.starts_with("【分步执行")
}

/// 与 `staged_plan_nl_followup_user_body` 注入正文首行一致；整段在展示层隐藏。
fn is_staged_nl_followup_bridge_user_content(s: &str) -> bool {
    s.trim_start()
        .starts_with(crate::runtime::plan_section::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX)
}

/// `user` 气泡 / CLI 用户侧展示。
pub(crate) fn user_message_for_chat_display(raw: &str) -> String {
    if !SHOW_STAGED_STEP_USER_BOILERPLATE_IN_CHAT && is_staged_step_injection_user_content(raw) {
        return String::new();
    }
    if is_staged_nl_followup_bridge_user_content(raw) {
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

/// 流式阶段检测到「正在输出 agent_reply_plan」时，聊天区不刷 JSON 片段；**仅展示围栏前自然语言**（无则空白，不占位文案）。**收齐后**走 [`assistant_markdown_source_for_display`] 再剥 JSON。
/// 模型若输出同义开场白，用 [`is_staged_plan_placeholder_like_line`] 识别并在展示中去掉，避免复读。
const STAGED_PLAN_STREAMING_PLACEHOLDER_BASE: &str = "正在生成分阶段规划";

fn triple_backtick_fence_count(s: &str) -> usize {
    s.match_indices("```").count()
}

/// 首段代码围栏（`parts[1]`）视为「JSON 规划流」：语言行为 `json` / `markdown` / `md`（与 [`strip_optional_json_fence_label`] 一致）且剥标后正文为空或 `{` 开头。
///
/// 不再把「无语言标签 + 内联 `{`」的裸围栏算作规划流：`deepseek-reasoner` 等思维链里常见的
/// ` ``` ` + `{`（讨论 JSON/代码）会触发 [`should_buffer_agent_reply_plan_stream`]，围栏前又无正文时聊天区会整段空白。
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

pub(crate) fn is_staged_plan_placeholder_like_line(line: &str) -> bool {
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
    // 与 `preprocess_unfenced_assistant_prose_dedup` 在「围栏前段落」上的 dedupe 对齐，避免流式与收齐后开场白不一致。
    let raw = crate::text_sanitize::dedupe_plain_assistant_preamble(&raw);
    // 与收齐后 `staged_plan_hidden_chat_prose_only` 一致：DSML、相邻重复行、列表并句，避免流式阶段出现双行复读而收齐后变单段等不一致。
    let prose_t = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw);
    let prose_t = prose_t.trim();
    if prose_t.is_empty() {
        String::new()
    } else {
        drop_leading_placeholder_like_prose_line(prose_t)
    }
}

/// TUI 状态栏：取 `staged_plan_notice` 首条**非占位**非空行（截断）；若无非占位行则空串（调用方回退仅模型名）。
pub(crate) fn staged_plan_notice_status_hint(text: &str) -> String {
    text.lines()
        .map(|l| l.trim_end())
        .find(|l| !l.trim().is_empty() && !is_staged_plan_placeholder_like_line(l))
        .unwrap_or("")
        .chars()
        .take(52)
        .collect()
}

/// 主聊天区隐藏 v1 规划列表时：
/// - 若围栏前有自然语言开场白，优先展示开场白（避免首句被覆盖）；
/// - 若只有纯 JSON，回退固定短提示（避免消息消失）。
fn staged_plan_hidden_chat_prose_only(original: &str) -> String {
    if let Ok(plan) = parse_agent_reply_plan_v1(original) {
        let raw_goal = prose_before_first_fence(original);
        let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
        let merged = augment_agent_reply_plan_goal_for_display(goal.trim(), &plan);
        let merged = merged.trim();
        if merged.is_empty() {
            format!("已生成分阶段规划（共 {} 步）。", plan.steps.len())
        } else {
            latex_math_to_unicode(merged)
        }
    } else {
        // 兜底：无法解析时沿用原有“围栏前自然语言”展示逻辑。
        let raw_goal = prose_before_first_fence(original);
        let goal = crate::text_sanitize::naturalize_assistant_plan_prose_tail(&raw_goal);
        let goal_t = goal.trim();
        if goal_t.is_empty() {
            String::new()
        } else {
            latex_math_to_unicode(goal_t)
        }
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
/// `SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT` 为 `false` 时：可解析为 v1 规划 → 不展示列表/JSON，
/// 优先展示围栏前开场白；纯 JSON 时回退固定短提示，避免消息隐藏。
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

/// 与流式 SSE 下发顺序一致：先 `reasoning_content` 再 `content`，中间无插入字符（与 `llm::api` 累加顺序一致）。
pub(crate) fn assistant_streaming_plain_concat(m: &Message) -> String {
    let mut s = String::new();
    if let Some(r) = m.reasoning_content.as_deref() {
        s.push_str(r);
    }
    if let Some(c) = crate::types::message_content_as_str(&m.content) {
        s.push_str(c);
    }
    s
}

/// `deepseek-reasoner` 等：拼接「思考过程」与正文为 Markdown 源，再走 [`assistant_markdown_source_for_display`]。
pub(crate) fn assistant_markdown_source_for_message(m: &Message) -> String {
    let raw = assistant_raw_markdown_body_from_parts(
        m.reasoning_content.as_deref().unwrap_or(""),
        crate::types::message_content_as_str(&m.content).unwrap_or(""),
    );
    assistant_markdown_source_for_display(&raw)
}

/// 展示用：有思维链时加小标题与分隔线，再拼接最终回答。
pub(crate) fn assistant_raw_markdown_body_from_parts(reasoning: &str, content: &str) -> String {
    let r = reasoning.trim();
    let c = content.trim();
    match (r.is_empty(), c.is_empty()) {
        (false, false) => format!("### 思考过程\n\n{r}\n\n---\n\n{c}"),
        (false, true) => format!("### 思考过程\n\n{r}"),
        (true, false) => c.to_string(),
        (true, true) => String::new(),
    }
}

/// 与 [`assistant_raw_markdown_body_from_parts`] 相同，从已组装的 [`Message`] 读取字段。
pub(crate) fn assistant_raw_markdown_body_for_message(m: &Message) -> String {
    assistant_raw_markdown_body_from_parts(
        m.reasoning_content.as_deref().unwrap_or(""),
        crate::types::message_content_as_str(&m.content).unwrap_or(""),
    )
}

/// 对助手正文做围栏前复读折叠：无围栏时整段处理；**有围栏时仍只处理首个 ` ``` ` 之前**，与流式阶段（`should_buffer` 前）一致。
fn preprocess_unfenced_assistant_prose_dedup(stripped: &str) -> String {
    let t = stripped.trim_start();
    if t.starts_with('{') {
        return stripped.to_string();
    }
    if let Some(idx) = stripped.find("```") {
        let (pre, from_fence) = stripped.split_at(idx);
        let pre_deduped = crate::text_sanitize::dedupe_plain_assistant_preamble(pre);
        format!("{pre_deduped}{from_fence}")
    } else {
        crate::text_sanitize::dedupe_plain_assistant_preamble(stripped)
    }
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
        let raw = crate::types::message_content_as_str(&messages[2].content).unwrap();
        let out = tool_content_for_display_for_message(raw, &messages, 2);
        assert_eq!(out, "ls");
        assert!(!out.contains(TOOL_OUTPUT_SECTION_HEADLINE));
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
        let out = assistant_markdown_source_for_display(raw);
        assert!(out.contains("已生成分阶段规划"));
        assert!(out.contains("共 1 步"));
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
            r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x"},{"id":"b","description":"y"}]}"#
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

    #[test]
    fn assistant_streaming_last_dedupes_duplicate_prose_before_partial_fence() {
        let line = "我将帮您编写一个简单的 C++ Hello World 程序，让我先规划任务步骤：";
        let raw = format!("{line}\n{line}\n```json\n{{\"type\":\"agent_reply_plan\"");
        let out = assistant_markdown_source_for_display_streaming_last(&raw);
        assert_eq!(out.matches(line).count(), 1, "{}", out);
        assert!(
            !out.contains("正在生成分阶段规划"),
            "流式阶段不在 UI 展示占位文案：{out:?}"
        );
    }

    #[test]
    fn assistant_streaming_last_buffers_partial_fenced_plan_json() {
        let raw = "下面拆解如下。\n```json\n{\"type\":\"agent_reply_plan\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.contains("下面拆解"));
        assert!(!out.contains("正在生成分阶段规划"));
        assert!(!out.contains("\"steps\""));
    }

    #[test]
    fn assistant_streaming_last_fence_prose_without_streaming_placeholder() {
        let raw = "这个任务可以分成以下步骤\n```json\n{\"type\":\"agent_reply_plan\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.contains("这个任务可以分成以下步骤"));
        assert!(!out.contains("正在生成分阶段规划"), "out={out:?}");
    }

    #[test]
    fn assistant_streaming_last_dedupes_placeholder_like_opening_line() {
        let raw = "正在生成分阶段规划...\n```json\n{\"type\":\"agent_reply_plan\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(
            out.is_empty(),
            "占位式开场白不展示，且无其它围栏前正文：{out:?}"
        );
    }

    #[test]
    fn assistant_streaming_last_dedupes_punctuation_variant_opening_lines() {
        let a = "我将先拆解任务步骤：";
        let b = "我将先拆解任务步骤。";
        let raw = format!("{a}\n{b}\n```json\n{{\"type\":\"agent_reply_plan\"");
        let out = assistant_markdown_source_for_display_streaming_last(&raw);
        assert_eq!(
            out.matches("我将先拆解任务步骤").count(),
            1,
            "开场白仅句末标点不同也应去重：{out:?}"
        );
        assert!(!out.contains("正在生成分阶段规划"));
    }

    #[test]
    fn assistant_streaming_last_plain_text_still_incremental() {
        let raw = "先写一句说明，再考虑格式。";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.contains("先写一句"));
        assert!(!out.contains("正在生成分阶段规划"));
    }

    /// `deepseek-reasoner` 思维链里常见无语言标签的围栏 + `{`，不得误判为 v1 规划缓冲而整段空白。
    #[test]
    fn assistant_streaming_last_bare_fence_json_like_not_buffered_to_empty() {
        let raw = "```\n{\"note\": \"thinking\"";
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(
            !out.trim().is_empty(),
            "裸 ``` 围栏不应触发规划缓冲清空：{out:?}"
        );
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
    fn assistant_streaming_last_whole_json_incomplete_shows_empty_until_complete() {
        let raw = r#"{"type":"agent_reply_plan","version":1,"steps":[{"id":"a","description":"x""#;
        let out = assistant_markdown_source_for_display_streaming_last(raw);
        assert!(out.is_empty(), "纯半截 JSON 流式阶段不占位文案：{out:?}");
    }

    #[test]
    fn staged_plan_notice_status_hint_skips_placeholder_lines() {
        assert_eq!(
            staged_plan_notice_status_hint("正在生成分阶段规划…\n步骤 1 进行中"),
            "步骤 1 进行中"
        );
        assert_eq!(staged_plan_notice_status_hint("正在生成分阶段规划…"), "");
    }

    #[test]
    fn user_hides_staged_step_injection_when_show_flag_false() {
        let raw = format!(
            "### 分步 1/2\n{}\n- id: s1\n- 描述: 读文件",
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE
        );
        assert_eq!(user_message_for_chat_display(&raw), "");
        let legacy = format!(
            "【分步执行 1/2】{}\n- id: s1\n- 描述: 读文件",
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE
        );
        assert_eq!(user_message_for_chat_display(&legacy), "");
    }

    #[test]
    fn user_hides_staged_nl_followup_bridge() {
        let raw = format!(
            "{}接下来你打算怎么帮我？简单用两三句说说就行。",
            crate::runtime::plan_section::STAGED_PLAN_NL_FOLLOWUP_USER_DISPLAY_HIDE_PREFIX
        );
        assert_eq!(user_message_for_chat_display(&raw), "");
    }
}
