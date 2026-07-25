//! 终端流工具过程：一行摘要 + 可选折叠详情（Phase 3）。

use crate::i18n::{self, Locale};
use crate::markdown::plaintext_to_safe_html;
use crate::message_format::{
    stored_tool_message_compact_text, stored_tool_message_detail_text, strip_ansi_codes,
};
use crate::storage::{StoredMessage, StoredMessageState};

const LIVE_TAIL_MAX_CHARS: usize = 120;

fn truncate_one_line(s: &str, max_chars: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let mut out: String = flat.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn tool_display_name(message: &StoredMessage) -> String {
    message
        .tool_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("tool")
        .to_string()
}

fn tool_status_label(message: &StoredMessage, locale: Locale) -> &'static str {
    if message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_loading)
    {
        return i18n::status_tool_running(locale);
    }
    if message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_error)
    {
        return i18n::chat_tui_tool_status_failed(locale);
    }
    i18n::chat_tui_tool_status_done(locale)
}

fn prepare_overlay_text(message: &StoredMessage, overlay: &str) -> String {
    if message.tool_name.as_deref() == Some("terminal_session") {
        strip_ansi_codes(overlay)
    } else {
        overlay.to_string()
    }
}

fn tool_summary_line(
    message: &StoredMessage,
    locale: Locale,
    live_output_overlay: Option<&str>,
) -> String {
    let mut compact = stored_tool_message_compact_text(message, locale);
    if compact.trim().is_empty()
        && let Some(overlay) = live_output_overlay.filter(|s| !s.is_empty())
    {
        compact = truncate_one_line(&prepare_overlay_text(message, overlay), LIVE_TAIL_MAX_CHARS);
    }
    if compact.trim().is_empty() {
        tool_display_name(message)
    } else {
        truncate_one_line(&compact, 180)
    }
}

fn tool_detail_body(
    message: &StoredMessage,
    locale: Locale,
    live_output_overlay: Option<&str>,
) -> String {
    let mut detail = stored_tool_message_detail_text(message, locale);
    if let Some(overlay) = live_output_overlay.filter(|s| !s.is_empty()) {
        let chunk = prepare_overlay_text(message, overlay);
        if detail.is_empty() {
            detail = chunk;
        } else if !detail.contains(chunk.trim()) {
            detail = format!("{detail}\n{chunk}");
        }
    }
    detail
}

/// 工具回合 body 内层 HTML（单块，不做按行 Markdown）。
#[must_use]
pub(crate) fn tool_process_body_html(
    message: &StoredMessage,
    locale: Locale,
    live_output_overlay: Option<&str>,
) -> String {
    let name = tool_display_name(message);
    let emoji = i18n::tool_kind_emoji(&name);
    let status = tool_status_label(message, locale);
    let summary = tool_summary_line(message, locale, live_output_overlay);
    let detail = tool_detail_body(message, locale, live_output_overlay);
    let mut html = String::new();
    html.push_str("<div class=\"chat-tui-tool-process\" data-testid=\"chat-tui-tool-process\">");
    html.push_str("<div class=\"chat-tui-tool-summary\">");
    html.push_str("<span class=\"chat-tui-tool-emoji\" aria-hidden=\"true\">");
    html.push_str(emoji);
    html.push_str("</span>");
    html.push_str("<span class=\"chat-tui-tool-name\">");
    html.push_str(&plaintext_to_safe_html(&name));
    html.push_str("</span>");
    html.push_str("<span class=\"chat-tui-tool-status\">");
    html.push_str(&plaintext_to_safe_html(status));
    html.push_str("</span>");
    html.push_str("<span class=\"chat-tui-tool-one-line\">");
    html.push_str(&plaintext_to_safe_html(&summary));
    html.push_str("</span></div>");
    let detail_trim = detail.trim();
    if !detail_trim.is_empty() && detail_trim != summary.trim() {
        html.push_str("<details class=\"chat-tui-tool-details\">");
        html.push_str("<summary>");
        html.push_str(&plaintext_to_safe_html(i18n::msg_tool_detail_expand_title(
            locale,
        )));
        html.push_str("</summary>");
        html.push_str("<pre class=\"chat-tui-tool-detail-body\">");
        html.push_str(&plaintext_to_safe_html(detail_trim));
        html.push_str("</pre></details>");
    }
    html.push_str("</div>");
    html
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn tool_msg(name: &str, text: &str, detail: &str, loading: bool) -> StoredMessage {
        StoredMessage {
            id: "t1".into(),
            role: "assistant".into(),
            text: text.into(),
            reasoning_text: detail.into(),
            image_urls: vec![],
            state: loading.then_some(StoredMessageState::Loading),
            is_tool: true,
            tool_call_id: Some("tc1".into()),
            tool_name: Some(name.into()),
            created_at: 0,
        }
    }

    #[test]
    fn loading_tool_shows_running_status() {
        let m = tool_msg("read_file", "读取中…", "", true);
        let html = tool_process_body_html(&m, Locale::ZhHans, None);
        assert!(html.contains("chat-tui-tool-process"), "{html}");
        assert!(html.contains("read_file"), "{html}");
        assert!(html.contains("工具执行中"), "{html}");
        assert!(!html.contains("<details"), "{html}");
    }

    #[test]
    fn finished_tool_with_detail_gets_details() {
        let m = tool_msg(
            "read_file",
            "读取成功",
            "fn main() {\n    println!(\"hi\");\n}",
            false,
        );
        let html = tool_process_body_html(&m, Locale::ZhHans, None);
        assert!(html.contains("chat-tui-tool-one-line"), "{html}");
        assert!(html.contains("<details"), "{html}");
        assert!(html.contains("println"), "{html}");
        assert!(html.contains("完成"), "{html}");
    }

    #[test]
    fn live_overlay_fills_empty_compact() {
        let m = tool_msg("run_command", "", "", true);
        let html = tool_process_body_html(&m, Locale::ZhHans, Some("line1\nline2"));
        assert!(html.contains("line1"), "{html}");
    }
}
