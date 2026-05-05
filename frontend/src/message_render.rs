//! 聊天相关 UI 写入 **`innerHTML`** 时的「展示用纯文本 → 安全 HTML」统一入口。

use crate::i18n::Locale;
use crate::markdown;
use crate::message_format::{
    assistant_text_for_display, assistant_thinking_body_and_answer_raw,
    filter_assistant_thinking_markers_for_display,
};

/// 助手非工具消息：流式/静态展示用的**纯文本**（与 `assistant_markdown_collapsible_view` 的 `Effect` 同源）。
#[allow(dead_code)]
pub fn assistant_body_plain_for_stream(
    reasoning_text: &str,
    text: &str,
    is_loading: bool,
    locale: Locale,
    apply_assistant_display_filters: bool,
) -> String {
    let filtered_reasoning_and_text = if apply_assistant_display_filters {
        Some((
            filter_assistant_thinking_markers_for_display(reasoning_text, is_loading),
            filter_assistant_thinking_markers_for_display(text, is_loading),
        ))
    } else {
        None
    };
    let (thinking_raw, answer_raw) = match &filtered_reasoning_and_text {
        Some((rs, tx)) => assistant_thinking_body_and_answer_raw(rs.as_str(), tx.as_str(), true),
        None => assistant_thinking_body_and_answer_raw(reasoning_text, text, false),
    };
    let r_trim = thinking_raw.trim();
    let answer_display = assistant_text_for_display(
        answer_raw,
        is_loading,
        locale,
        apply_assistant_display_filters,
    );
    if r_trim.is_empty() {
        answer_display
    } else if answer_display.trim().is_empty() {
        r_trim.to_string()
    } else {
        format!("{r_trim}\n\n{answer_display}")
    }
}

/// 将片段转为可写入 `innerHTML` 的安全 HTML（Markdown 开/关与设置 **`GET /web-ui`** 对齐）。
pub fn fragment_to_chat_safe_html(fragment: &str, markdown_render: bool) -> String {
    if fragment.trim().is_empty() {
        return String::new();
    }
    if markdown_render {
        markdown::to_safe_html(fragment)
    } else {
        markdown::plaintext_to_safe_html(fragment)
    }
}

#[cfg(test)]
mod tests {
    use super::{assistant_body_plain_for_stream, fragment_to_chat_safe_html};
    use crate::i18n::Locale;

    #[test]
    fn fragment_md_on_parses_table() {
        let h = fragment_to_chat_safe_html("|a|b|\n|---|---|\n|1|2|", true);
        assert!(h.contains("<table"), "expected table, got {h:?}");
    }

    #[test]
    fn fragment_md_off_escapes_angle_brackets() {
        let h = fragment_to_chat_safe_html("<not-a-tag>", false);
        assert!(h.contains("&lt;"), "expected escape, got {h:?}");
    }

    #[test]
    fn assistant_plain_then_fragment_emits_bold() {
        let plain = assistant_body_plain_for_stream("", "Hello **x**", false, Locale::En, true);
        let h = fragment_to_chat_safe_html(&plain, true);
        assert!(
            h.contains("<strong>") || h.contains("<b>"),
            "expected bold, got {h:?}"
        );
    }
}
