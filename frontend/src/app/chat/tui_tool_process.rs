//! 终端流工具过程：一行摘要 + 可选折叠详情（Phase 3）。

use crate::i18n::{self, Locale};
use crate::markdown::plaintext_to_safe_html;
use crate::message_format::{
    stored_tool_message_compact_text, stored_tool_message_detail_text, strip_ansi_codes,
};
use crate::storage::{StoredMessage, StoredMessageState};
use crate::timeline_scan::timeline_tool_ok;
use crabmate_tool_card::{ToolCardLocale, tool_signal_beside_tool};

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

fn tool_row_outcome(message: &StoredMessage) -> ToolRowOutcome {
    if message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_loading)
    {
        return ToolRowOutcome::Running;
    }
    if message
        .state
        .as_ref()
        .is_some_and(StoredMessageState::is_error)
    {
        return ToolRowOutcome::Failed;
    }
    // 工具结果写入的是 TimelineUiJson{ok}，不是 StoredMessageState::Error。
    if message
        .state
        .as_ref()
        .and_then(timeline_tool_ok)
        .is_some_and(|ok| !ok)
    {
        return ToolRowOutcome::Failed;
    }
    // 流结束/重启收口后 `state` 已清，靠 reasoning 保留的 status 行区分完成与中断。
    if message.reasoning_text.contains("status: stopped (user)") {
        return ToolRowOutcome::StoppedUser;
    }
    if message
        .reasoning_text
        .contains("status: interrupted (stale)")
    {
        return ToolRowOutcome::InterruptedStale;
    }
    if tool_compact_looks_failed(message) {
        return ToolRowOutcome::Failed;
    }
    ToolRowOutcome::Done
}

/// 无 timeline `ok` 时的兜底：失败标题前缀或非 0 `(exit=N)`。
fn tool_compact_looks_failed(message: &StoredMessage) -> bool {
    let text = message.text.trim();
    if text.is_empty() {
        return false;
    }
    let name = tool_display_name(message);
    if text.starts_with(&format!("{name}失败"))
        || text.starts_with(&format!("{name} failed"))
        || text.starts_with("命令执行失败")
        || text.starts_with("Command run failed")
    {
        return true;
    }
    non_zero_exit_in_parens(text)
}

fn non_zero_exit_in_parens(text: &str) -> bool {
    let Some(rest) = text.split_once("(exit=").map(|(_, r)| r) else {
        return false;
    };
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return false;
    }
    digits.parse::<i32>().is_ok_and(|n| n != 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ToolRowOutcome {
    Running,
    Done,
    Failed,
    StoppedUser,
    InterruptedStale,
}

impl ToolRowOutcome {
    /// 行首状态符（固定宽槽，避免 ⏳→✅/⚠️ 切换时布局跳动）。
    fn mark(self) -> &'static str {
        match self {
            Self::Running => "⏳",
            Self::Done => "✅",
            Self::Failed | Self::StoppedUser | Self::InterruptedStale => "⚠️",
        }
    }

    fn aria_label(self, locale: Locale) -> &'static str {
        match self {
            Self::Running => i18n::status_tool_running(locale),
            Self::Done => i18n::chat_tui_tool_status_done(locale),
            Self::Failed => i18n::chat_tui_tool_status_failed(locale),
            Self::StoppedUser => i18n::status_tool_stopped_user(locale),
            Self::InterruptedStale => i18n::status_tool_interrupted_stale(locale),
        }
    }
}

fn prepare_overlay_text(message: &StoredMessage, overlay: &str) -> String {
    if message.tool_name.as_deref() == Some("terminal_session") {
        strip_ansi_codes(overlay)
    } else {
        overlay.to_string()
    }
}

fn tool_card_locale(locale: Locale) -> ToolCardLocale {
    match locale {
        Locale::ZhHans => ToolCardLocale::ZhHans,
        Locale::En => ToolCardLocale::En,
    }
}

fn tool_summary_line(
    message: &StoredMessage,
    locale: Locale,
    live_output_overlay: Option<&str>,
) -> String {
    let name = tool_display_name(message);
    let mut compact = stored_tool_message_compact_text(message, locale);
    if compact.trim().is_empty()
        && let Some(overlay) = live_output_overlay.filter(|s| !s.is_empty())
    {
        compact = truncate_one_line(&prepare_overlay_text(message, overlay), LIVE_TAIL_MAX_CHARS);
    }
    // 工具名已在 `.chat-tui-tool-name`；勿把 compact 里的 id / 人类名再拼进 one-line。
    tool_signal_beside_tool(&name, &compact, tool_card_locale(locale))
        .map(|s| truncate_one_line(&s, 180))
        .unwrap_or_default()
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
    /// 行首状态符（⏳ 执行中 / ✅ 完成 / ⚠️ 失败与中断）。
    pub status: String,
    /// `aria-label` / `title` 可读文案（完成 / 失败 / 执行中…）。
    pub status_label: String,
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
    let outcome = tool_row_outcome(message);
    ToolRowLiveFields {
        status: outcome.mark().to_string(),
        status_label: outcome.aria_label(locale).to_string(),
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
        "<span class=\"chat-tui-tool-status\" aria-label=\"{label}\" title=\"{label}\">{status}</span>\
         <span class=\"chat-tui-tool-emoji\" aria-hidden=\"true\">{emoji}</span>\
         <span class=\"chat-tui-tool-name\">{name}</span>\
         <span class=\"chat-tui-tool-one-line\">{one}</span>",
        label = plaintext_to_safe_html(&fields.status_label),
        status = plaintext_to_safe_html(&fields.status),
        emoji = emoji,
        name = plaintext_to_safe_html(&name),
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
        let fields = tool_row_live_fields(&m, Locale::ZhHans, None);
        assert_eq!(fields.status, "⏳");
        assert!(
            fields.status_label.contains("执行中"),
            "{}",
            fields.status_label
        );
        let html = tool_process_body_html(&m, Locale::ZhHans, None);
        assert!(html.contains("chat-tui-tool-process"), "{html}");
        assert!(html.contains("chat-tui-tool-row"), "{html}");
        assert!(html.contains("read_file"), "{html}");
        assert!(html.contains("⏳"), "{html}");
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
        assert_eq!(fields.status, "⚠️");
        assert_eq!(fields.status_label, "已中断");
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
        assert!(html.contains("✅"), "{html}");
        assert!(html.contains("aria-label=\"完成\""), "{html}");
    }

    #[test]
    fn failed_and_interrupted_share_warning_mark() {
        let failed = StoredMessage {
            id: "t1".into(),
            role: "assistant".into(),
            text: "失败".into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(StoredMessageState::Error),
            is_tool: true,
            tool_call_id: Some("tc1".into()),
            tool_name: Some("run_command".into()),
            created_at: 0,
        };
        let fields = tool_row_live_fields(&failed, Locale::ZhHans, None);
        assert_eq!(fields.status, "⚠️");
        assert_eq!(fields.status_label, "失败");
    }

    #[test]
    fn timeline_ok_false_shows_warning_not_check() {
        use crate::timeline_scan::timeline_state_tool;
        let m = StoredMessage {
            id: "t1".into(),
            role: "system".into(),
            text: "cargo_check失败 cargo check (exit=101)".into(),
            reasoning_text: "exit=101".into(),
            image_urls: vec![],
            state: Some(timeline_state_tool("t1", false)),
            is_tool: true,
            tool_call_id: Some("tc1".into()),
            tool_name: Some("cargo_check".into()),
            created_at: 0,
        };
        let fields = tool_row_live_fields(&m, Locale::ZhHans, None);
        assert_eq!(fields.status, "⚠️");
        assert_eq!(fields.status_label, "失败");
        assert_eq!(fields.one_line, "cargo check (exit=101)");
        assert!(!fields.one_line.contains("失败"), "{:?}", fields.one_line);
    }

    #[test]
    fn live_overlay_fills_empty_compact() {
        let m = tool_msg("run_command", "", "", true);
        let html = tool_process_body_html(&m, Locale::ZhHans, Some("line1\nline2"));
        assert!(html.contains("line1"), "{html}");
    }

    #[test]
    fn one_line_strips_redundant_tool_name_keeps_paren_mode() {
        let m = tool_msg(
            "git_diff_stat",
            "git_diff_stat (working)",
            "stat output",
            false,
        );
        let fields = tool_row_live_fields(&m, Locale::ZhHans, None);
        assert_eq!(fields.one_line, "(working)");
        let html = tool_process_body_html(&m, Locale::ZhHans, None);
        let name_count = html.matches("git_diff_stat").count();
        assert_eq!(
            name_count, 1,
            "工具名只应出现在 name 槽，不应再进 one-line: {html}"
        );
        assert!(html.contains("(working)"), "{html}");
        assert!(html.contains("✅"), "{html}");
        assert!(html.contains("aria-label=\"完成\""), "{html}");
    }

    #[test]
    fn empty_compact_does_not_echo_tool_name_in_one_line() {
        let m = tool_msg("git_status", "", "", true);
        let fields = tool_row_live_fields(&m, Locale::ZhHans, None);
        assert!(
            fields.one_line.is_empty(),
            "空 compact 不应回退成工具名: {:?}",
            fields.one_line
        );
    }

    #[test]
    fn run_command_one_line_strips_human_title() {
        let m = tool_msg(
            "run_command",
            "命令执行 cargo clippy --manifest-path frontend/Cargo.toml --all-targets --all-features -- -D warnings",
            "ok",
            false,
        );
        let fields = tool_row_live_fields(&m, Locale::ZhHans, None);
        assert_eq!(
            fields.one_line,
            "cargo clippy --manifest-path frontend/Cargo.toml --all-targets --all-features -- -D warnings"
        );
        assert!(
            !fields.one_line.contains("命令执行"),
            "{:?}",
            fields.one_line
        );
        let html = tool_process_body_html(&m, Locale::ZhHans, None);
        // 冒号由 CSS `:has(+ .chat-tui-tool-one-line:not(:empty))::after` 在有摘要时加上。
        assert!(
            html.contains("chat-tui-tool-name") && html.contains("chat-tui-tool-one-line"),
            "{html}"
        );
        assert!(html.contains("cargo clippy"), "{html}");
    }
}
