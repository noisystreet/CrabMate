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
    // 流结束/重启收口后 `state` 已清，靠 reasoning 保留的 status 行区分「完成」与中断。
    if message.reasoning_text.contains("status: stopped (user)") {
        return i18n::status_tool_stopped_user(locale);
    }
    if message
        .reasoning_text
        .contains("status: interrupted (stale)")
    {
        return i18n::status_tool_interrupted_stale(locale);
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

/// 工具折叠行可增量更新的字段（与 DOM `.chat-tui-tool-status` / `.chat-tui-tool-one-line` 对应）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ToolRowLiveFields {
    pub status: String,
    pub one_line: String,
    /// 非空则折叠行带 `<details>`（结构变化时须整段 ReplaceAll）。
    pub detail: Option<String>,
}

impl ToolRowLiveFields {
    #[inline]
    pub(crate) fn wants_details(&self) -> bool {
        self.detail.is_some()
    }
}

/// 从工具消息提取 live 行字段。
#[must_use]
pub(crate) fn tool_row_live_fields(
    message: &StoredMessage,
    locale: Locale,
    live_output_overlay: Option<&str>,
) -> ToolRowLiveFields {
    let summary = tool_summary_line(message, locale, live_output_overlay);
    let detail = tool_detail_body(message, locale, live_output_overlay);
    let detail_trim = detail.trim();
    let detail = if !detail_trim.is_empty() && detail_trim != summary.trim() {
        Some(detail_trim.to_string())
    } else {
        None
    };
    ToolRowLiveFields {
        status: tool_status_label(message, locale).to_string(),
        one_line: summary,
        detail,
    }
}

/// 工具回合 body 内层 HTML（折叠态单行固定高度；详情展开后才增高）。
#[must_use]
pub(crate) fn tool_process_body_html(
    message: &StoredMessage,
    locale: Locale,
    live_output_overlay: Option<&str>,
) -> String {
    let name = tool_display_name(message);
    let emoji = i18n::tool_kind_emoji(&name);
    let fields = tool_row_live_fields(message, locale, live_output_overlay);
    let row_inner = format!(
        "<span class=\"chat-tui-tool-emoji\" aria-hidden=\"true\">{emoji}</span>\
         <span class=\"chat-tui-tool-name\">{name}</span>\
         <span class=\"chat-tui-tool-status\">{status}</span>\
         <span class=\"chat-tui-tool-one-line\">{one}</span>",
        emoji = emoji,
        name = plaintext_to_safe_html(&name),
        status = plaintext_to_safe_html(&fields.status),
        one = plaintext_to_safe_html(&fields.one_line),
    );
    let mut html = String::new();
    html.push_str("<div class=\"chat-tui-tool-process\" data-testid=\"chat-tui-tool-process\">");
    if let Some(detail_trim) = fields.detail.as_deref() {
        // summary 即整行工具条；展开后 pre 落在固定行之外。
        html.push_str("<details class=\"chat-tui-tool-details\">");
        html.push_str("<summary class=\"chat-tui-tool-row\" title=\"");
        html.push_str(&plaintext_to_safe_html(i18n::msg_tool_detail_expand_title(
            locale,
        )));
        html.push_str("\">");
        html.push_str(&row_inner);
        html.push_str("<span class=\"chat-tui-tool-expand\" aria-hidden=\"true\">▸</span>");
        html.push_str("</summary>");
        html.push_str("<pre class=\"chat-tui-tool-detail-body\">");
        html.push_str(&plaintext_to_safe_html(detail_trim));
        html.push_str("</pre></details>");
    } else {
        html.push_str("<div class=\"chat-tui-tool-row\">");
        html.push_str(&row_inner);
        html.push_str("</div>");
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
        assert!(html.contains("chat-tui-tool-row"), "{html}");
        assert!(html.contains("read_file"), "{html}");
        assert!(html.contains("工具执行中"), "{html}");
        assert!(!html.contains("<details"), "{html}");
    }

    #[test]
    fn interrupted_stale_status_label_not_done() {
        let message = StoredMessage {
            id: "t1".into(),
            role: "system".into(),
            text: "已中断 · 工具：http_fetch".into(),
            reasoning_text: "tool: http_fetch\nstatus: interrupted (stale)".into(),
            image_urls: vec![],
            state: None,
            is_tool: true,
            tool_call_id: None,
            tool_name: Some("http_fetch".into()),
            created_at: 0,
        };
        let fields = tool_row_live_fields(&message, Locale::ZhHans, None);
        assert_eq!(fields.status, "已中断");
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
        assert!(
            html.contains("summary class=\"chat-tui-tool-row\""),
            "{html}"
        );
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
