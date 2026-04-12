//! 聊天相关 UI 写入 **`innerHTML`** 时的「展示用纯文本 → 安全 HTML」统一入口。
//!
//! ## 职责
//!
//! - **[`fragment_to_chat_safe_html`]**：已拼好的 Markdown/纯文本片段，按 **`markdown_render`**
//!   调用 [`crate::markdown::to_safe_html`] 或 [`crate::markdown::plaintext_to_safe_html`]。
//!   用于助手流式气泡终帧、工作区**变更集**模态等。
//! - **[`assistant_body_plain_for_stream`]**：与 [`crate::assistant_body`] 内流式 **`Effect`** 一致，
//!   将 **`reasoning_text` + `text`** 经 [`crate::message_format`] 助手过滤链压成一段纯文本；
//!   再与 [`fragment_to_chat_safe_html`] 组合即完整渲染链。
//!
//! ## 约定
//!
//! 业务组件应通过本模块访问上述 HTML 路径，避免散落直接调用 [`crate::markdown::to_safe_html`]，
//! 便于日后增加统一前后处理（若需要）。

use crate::i18n::Locale;
use crate::markdown;
use crate::message_format::{
    assistant_text_for_display, assistant_thinking_body_and_answer_raw,
    filter_assistant_thinking_markers_for_display,
};

/// 与 `assistant_body` 一致：思维链 trim 后与终答拼成一段（无单独「思考过程」容器）。
fn combined_assistant_display_plain(thinking_trimmed: &str, answer_display: &str) -> String {
    if thinking_trimmed.is_empty() {
        return answer_display.to_string();
    }
    if answer_display.trim().is_empty() {
        return thinking_trimmed.to_string();
    }
    format!("{thinking_trimmed}\n\n{answer_display}")
}

/// 助手非工具消息：流式/静态展示用的**纯文本**（与 `assistant_markdown_collapsible_view` 的 `Effect` 同源）。
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
    combined_assistant_display_plain(r_trim, answer_display.as_str())
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
