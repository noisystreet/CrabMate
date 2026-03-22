//! 对话消息在 UI/终端上的展示用正文（与 `Message.content` 存储形态解耦）。

use regex::Regex;
use std::sync::LazyLock;

use crate::agent::plan_artifact::format_agent_reply_plan_for_display;
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
}
