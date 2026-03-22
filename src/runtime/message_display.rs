//! 对话消息在 UI/终端上的展示用正文（与 `Message.content` 存储形态解耦）。

use regex::Regex;
use std::sync::LazyLock;

use crate::agent::plan_artifact::{format_agent_reply_plan_for_display, parse_agent_reply_plan_v1};
use crate::runtime::latex_unicode::latex_math_to_unicode;
/// `role: tool` 的 `content` 若为 JSON 且含 `human_summary`，与 TUI 一致优先展示该字段；否则回退为原文。
pub(crate) fn tool_content_for_display(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|v| {
            v.get("human_summary")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| raw.to_string())
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

/// 助手气泡 / CLI ANSI / 导出共用：剥标签 → `agent_reply_plan` 可读化 → LaTeX。
pub(crate) fn assistant_markdown_source_for_display(raw: &str) -> String {
    let stripped = strip_assistant_echo_label(raw);
    if !SHOW_STAGED_PLAN_PHASE_ASSISTANT_IN_CHAT && parse_agent_reply_plan_v1(&stripped).is_ok() {
        return String::new();
    }
    let display = format_agent_reply_plan_for_display(&stripped).unwrap_or(stripped);
    latex_math_to_unicode(&display)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_prefers_human_summary() {
        let raw = r#"{"human_summary":"编译成功","ok":true}"#;
        assert_eq!(tool_content_for_display(raw), "编译成功");
    }

    #[test]
    fn tool_non_json_is_passthrough() {
        let raw = "plain tool output";
        assert_eq!(tool_content_for_display(raw), "plain tool output");
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
    fn user_hides_staged_step_injection_when_show_flag_false() {
        let raw = format!(
            "【分步执行 1/2】{}\n- id: s1\n- 描述: 读文件",
            crate::runtime::plan_section::STAGED_STEP_USER_BOILERPLATE
        );
        assert_eq!(user_message_for_chat_display(&raw), "");
    }
}
