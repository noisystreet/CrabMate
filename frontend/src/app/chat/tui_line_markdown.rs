//! 终端流按**块** Markdown：已闭合块做安全 HTML（冻结，增量只 append）；
//! 活跃末块做**流式安全**行内增强（成对 `**` / `` ` `` / `~~` 才着色；半截标记保持字面量）；
//! 未闭合围栏仍纯文本。段落/表格/列表在空行或块类型切换时才冻结，避免终态全文重渲抖动。

use crate::markdown::{plaintext_to_safe_html, to_safe_html};

/// 活跃行是否为未闭合围栏缓冲（须 `textContent`，禁止行内 HTML）。
#[must_use]
pub fn open_line_is_fence_buffer(text: &str) -> bool {
    text.starts_with("```")
}

fn push_escaped_char(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        '"' => out.push_str("&quot;"),
        _ => out.push(c),
    }
}

fn push_escaped_str(out: &mut String, s: &str) {
    for c in s.chars() {
        push_escaped_char(out, c);
    }
}

/// 在 `from` 起查找闭合 delimiter；要求非空内容且不跨换行。
fn find_closing_delim(s: &str, from: usize, delim: &str) -> Option<usize> {
    if from >= s.len() {
        return None;
    }
    let rest = &s[from..];
    let mut search_at = 0usize;
    while let Some(rel) = rest[search_at..].find(delim) {
        let abs = from + search_at + rel;
        if abs > from {
            let inner = &s[from..abs];
            if !inner.is_empty() && !inner.contains('\n') {
                return Some(abs);
            }
        }
        search_at += rel + delim.len();
        if search_at >= rest.len() {
            break;
        }
    }
    None
}

/// 流式活跃行：仅渲染**已成对**的行内标记，半截 `**` / `` ` `` 保持转义字面量。
/// 不做标题/列表/围栏（那些等块闭合后再 `to_safe_html`）。
#[must_use]
pub fn stream_inline_safe_html(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    let mut i = 0usize;
    while i < text.len() {
        if text[i..].starts_with('`') {
            if let Some(end) = find_closing_delim(text, i + 1, "`") {
                out.push_str("<code>");
                push_escaped_str(&mut out, &text[i + 1..end]);
                out.push_str("</code>");
                i = end + 1;
                continue;
            }
            push_escaped_char(&mut out, '`');
            i += '`'.len_utf8();
            continue;
        }
        if text[i..].starts_with("**") {
            if let Some(end) = find_closing_delim(text, i + 2, "**") {
                out.push_str("<strong>");
                out.push_str(&stream_inline_safe_html_no_strong(&text[i + 2..end]));
                out.push_str("</strong>");
                i = end + 2;
                continue;
            }
            push_escaped_str(&mut out, "**");
            i += 2;
            continue;
        }
        if text[i..].starts_with("~~") {
            if let Some(end) = find_closing_delim(text, i + 2, "~~") {
                out.push_str("<del>");
                push_escaped_str(&mut out, &text[i + 2..end]);
                out.push_str("</del>");
                i = end + 2;
                continue;
            }
            push_escaped_str(&mut out, "~~");
            i += 2;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or('\0');
        push_escaped_char(&mut out, c);
        i += c.len_utf8();
    }
    out
}

/// 粗体内只处理 code / 转义，避免 `**` 递归。
fn stream_inline_safe_html_no_strong(text: &str) -> String {
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    let mut i = 0usize;
    while i < text.len() {
        if text[i..].starts_with('`') {
            if let Some(end) = find_closing_delim(text, i + 1, "`") {
                out.push_str("<code>");
                push_escaped_str(&mut out, &text[i + 1..end]);
                out.push_str("</code>");
                i = end + 1;
                continue;
            }
            push_escaped_char(&mut out, '`');
            i += 1;
            continue;
        }
        let c = text[i..].chars().next().unwrap_or('\0');
        push_escaped_char(&mut out, c);
        i += c.len_utf8();
    }
    out
}

/// 活跃块写入 DOM / `to_inner_html`。
///
/// `markdown_render=false` 时全程纯文本转义（含活跃行，对齐 `CM_WEB_DISABLE_MARKDOWN`）。
#[must_use]
pub fn render_open_active_html(text: &str, markdown_render: bool) -> String {
    if !markdown_render || open_line_is_fence_buffer(text) {
        return plaintext_to_safe_html(text);
    }
    if !text.contains('\n') {
        return stream_inline_safe_html(text);
    }
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<br />");
        }
        out.push_str(&stream_inline_safe_html(line));
    }
    out
}

fn is_fence_marker(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

fn fence_info(line: &str) -> &str {
    line.trim_start()
        .strip_prefix("```")
        .map(str::trim)
        .unwrap_or("")
}

fn blank_line_html() -> String {
    "<div class=\"chat-tui-line chat-tui-line--blank\"><br /></div>".to_string()
}

fn wrap_closed_html(inner: &str) -> String {
    if inner.is_empty() {
        return blank_line_html();
    }
    format!("<div class=\"chat-tui-line chat-tui-line--block\">{inner}</div>")
}

fn closed_md_html(src: &str, markdown_render: bool) -> String {
    let html = if markdown_render {
        to_safe_html(src)
    } else {
        plaintext_to_safe_html(src)
    };
    wrap_closed_html(&html)
}

fn fence_html(lang: &str, body: &str, markdown_render: bool) -> String {
    let mut fenced = String::with_capacity(body.len() + lang.len() + 16);
    fenced.push_str("```");
    fenced.push_str(lang);
    fenced.push('\n');
    fenced.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        fenced.push('\n');
    }
    fenced.push_str("```");
    let html = if markdown_render {
        to_safe_html(&fenced)
    } else {
        plaintext_to_safe_html(&fenced)
    };
    format!("<div class=\"chat-tui-line chat-tui-line--fence\">{html}</div>")
}

fn open_fence_plain_text(lang: &str, body: &str, open_tail: &str) -> String {
    let mut plain = String::new();
    plain.push_str("```");
    plain.push_str(lang);
    plain.push('\n');
    plain.push_str(body);
    plain.push_str(open_tail);
    plain
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Paragraph,
    Table,
    List,
}

fn is_table_line(line: &str) -> bool {
    // 仅 GFM 管道表常见形态，避免「a | b」散文误判为表格。
    line.trim_start().starts_with('|')
}

fn is_list_line(line: &str) -> bool {
    let t = line.trim_start();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return true;
    }
    let mut chars = t.chars().peekable();
    let mut saw_digit = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            saw_digit = true;
            chars.next();
            continue;
        }
        break;
    }
    if !saw_digit {
        return false;
    }
    match chars.next() {
        Some('.') | Some(')') => chars.next() == Some(' '),
        _ => false,
    }
}

fn is_atx_heading_line(line: &str) -> bool {
    let t = line.trim_start();
    let n = t.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&n) && t.as_bytes().get(n) == Some(&b' ')
}

fn is_thematic_break_line(line: &str) -> bool {
    let t = line.trim();
    if t.len() < 3 {
        return false;
    }
    let b = t.as_bytes()[0];
    if b != b'-' && b != b'*' && b != b'_' {
        return false;
    }
    t.bytes().all(|c| c == b || c == b' ')
}

fn is_block_continuation(line: &str) -> bool {
    line.starts_with("    ") || line.starts_with('\t')
}

fn classify_block_line(line: &str) -> BlockKind {
    if is_table_line(line) {
        BlockKind::Table
    } else if is_list_line(line) {
        BlockKind::List
    } else {
        BlockKind::Paragraph
    }
}

fn push_pending_line(pending: &mut String, line: &str) {
    if !pending.is_empty() {
        pending.push('\n');
    }
    pending.push_str(line);
}

fn flush_pending(
    closed: &mut Vec<String>,
    pending: &mut String,
    pending_kind: &mut Option<BlockKind>,
    markdown_render: bool,
) {
    if pending.is_empty() {
        *pending_kind = None;
        return;
    }
    closed.push(closed_md_html(pending, markdown_render));
    pending.clear();
    *pending_kind = None;
}

/// 按块解析结果：闭合块 HTML 列表（冻结）+ 可选活跃块**源文本**（渲染见 [`render_open_active_html`]）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TuiBodyChunks {
    pub closed: Vec<String>,
    /// 活跃块源文本；围栏缓冲以 \`\`\` 开头。DOM 用 [`render_open_active_html`] 写入。
    pub open_plain: Option<String>,
    /// 与解析时 `markdown_render` 一致；活跃行渲染须读取此标志。
    pub markdown_render: bool,
}

impl Default for TuiBodyChunks {
    fn default() -> Self {
        Self {
            closed: Vec::new(),
            open_plain: None,
            markdown_render: true,
        }
    }
}

fn open_active_line_class(text: &str, markdown_render: bool) -> &'static str {
    if !markdown_render || open_line_is_fence_buffer(text) {
        "chat-tui-line chat-tui-line--plain"
    } else {
        "chat-tui-line chat-tui-line--active"
    }
}

impl TuiBodyChunks {
    #[must_use]
    pub fn to_inner_html(&self) -> String {
        let mut out = String::new();
        for chunk in &self.closed {
            out.push_str(chunk);
        }
        if let Some(plain) = &self.open_plain {
            out.push_str("<div class=\"");
            out.push_str(open_active_line_class(plain, self.markdown_render));
            out.push_str("\">");
            out.push_str(&render_open_active_html(plain, self.markdown_render));
            out.push_str("</div>");
        }
        out
    }
}

/// live body 增量：优先 append **冻结**闭合块 + 只改活跃块；工具行改 status/one-line；否则整 body 替换。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TuiBodyPatch {
    ReplaceAll {
        chunks: TuiBodyChunks,
    },
    Incremental {
        append_closed: Vec<String>,
        /// 活跃块源文本（非 HTML）；`None` 表示移除活跃行节点。
        open_plain: Option<String>,
        markdown_render: bool,
    },
    /// 工具折叠行：只改文案，不重写 HTML（高度与结构保持不变）。
    ToolRow {
        status: String,
        one_line: String,
        /// 若 DOM 已有 details，同步更新详情正文；`None` 表示无详情块。
        detail: Option<String>,
    },
}

struct BlockAbsorbState {
    closed: Vec<String>,
    pending: String,
    pending_kind: Option<BlockKind>,
    in_fence: bool,
    fence_lang: String,
    fence_body: String,
    markdown_render: bool,
}

impl BlockAbsorbState {
    fn new(markdown_render: bool) -> Self {
        Self {
            closed: Vec::new(),
            pending: String::new(),
            pending_kind: None,
            in_fence: false,
            fence_lang: String::new(),
            fence_body: String::new(),
            markdown_render,
        }
    }

    fn flush_pending(&mut self) {
        flush_pending(
            &mut self.closed,
            &mut self.pending,
            &mut self.pending_kind,
            self.markdown_render,
        );
    }

    fn absorb_complete_line(&mut self, line: &str) {
        if is_fence_marker(line) {
            self.flush_pending();
            if self.in_fence {
                self.closed.push(fence_html(
                    &self.fence_lang,
                    &self.fence_body,
                    self.markdown_render,
                ));
                self.in_fence = false;
                self.fence_lang.clear();
                self.fence_body.clear();
            } else {
                self.in_fence = true;
                self.fence_lang = fence_info(line).to_string();
                self.fence_body.clear();
            }
            return;
        }
        if self.in_fence {
            self.fence_body.push_str(line);
            self.fence_body.push('\n');
            return;
        }
        if line.trim().is_empty() {
            self.flush_pending();
            self.closed.push(blank_line_html());
            return;
        }
        if is_atx_heading_line(line) || is_thematic_break_line(line) {
            self.flush_pending();
            self.closed.push(closed_md_html(line, self.markdown_render));
            return;
        }
        let kind = classify_block_line(line);
        match self.pending_kind {
            None => {
                push_pending_line(&mut self.pending, line);
                self.pending_kind = Some(kind);
            }
            Some(prev)
                if prev == kind
                    || (prev == BlockKind::List && is_block_continuation(line))
                    || (prev == BlockKind::Paragraph && is_block_continuation(line)) =>
            {
                push_pending_line(&mut self.pending, line);
            }
            Some(_) => {
                self.flush_pending();
                push_pending_line(&mut self.pending, line);
                self.pending_kind = Some(kind);
            }
        }
    }
}

/// 将正文解析为可挂载的 closed / open 块。
///
/// `markdown_render=false` 时闭合块与围栏均走纯文本转义（对齐 `CM_WEB_DISABLE_MARKDOWN`）。
#[must_use]
pub fn parse_tui_body_chunks_with(
    text: &str,
    finalize_open_line: bool,
    markdown_render: bool,
) -> TuiBodyChunks {
    if text.is_empty() {
        return TuiBodyChunks::default();
    }

    let ends_with_nl = text.ends_with('\n');
    let raw: Vec<&str> = text.split('\n').collect();
    let (complete, open_line): (&[&str], Option<&str>) = if ends_with_nl {
        let n = raw.len().saturating_sub(1);
        (&raw[..n], None)
    } else if raw.is_empty() {
        (&[], None)
    } else {
        let last = raw.len() - 1;
        (&raw[..last], Some(raw[last]))
    };

    let mut state = BlockAbsorbState::new(markdown_render);
    for line in complete {
        state.absorb_complete_line(line);
    }

    let open_plain = match open_line {
        Some(open) if state.in_fence => Some(open_fence_plain_text(
            &state.fence_lang,
            &state.fence_body,
            open,
        )),
        Some(open) if finalize_open_line => {
            if is_fence_marker(open) {
                push_pending_line(&mut state.pending, open);
            } else if open.trim().is_empty() {
                state.flush_pending();
                state.closed.push(blank_line_html());
            } else {
                push_pending_line(&mut state.pending, open);
            }
            state.flush_pending();
            None
        }
        Some(open) => {
            let mut open_buf = std::mem::take(&mut state.pending);
            let _ = state.pending_kind.take();
            push_pending_line(&mut open_buf, open);
            Some(open_buf)
        }
        None if state.in_fence => Some(open_fence_plain_text(
            &state.fence_lang,
            &state.fence_body,
            "",
        )),
        None if !state.pending.is_empty() => {
            // 完整行已到、尚无空行收束：留在 open，避免提前冻结导致后续同段续写时 ReplaceAll。
            Some(std::mem::take(&mut state.pending))
        }
        None => None,
    };

    TuiBodyChunks {
        closed: state.closed,
        open_plain,
        markdown_render,
    }
}

/// 对比上一帧 closed 前缀：可增量则 append，否则整段替换。
#[must_use]
pub fn plan_tui_body_patch(prev: Option<&TuiBodyChunks>, next: &TuiBodyChunks) -> TuiBodyPatch {
    let Some(prev) = prev else {
        return TuiBodyPatch::ReplaceAll {
            chunks: next.clone(),
        };
    };
    if next.markdown_render == prev.markdown_render
        && next.closed.len() >= prev.closed.len()
        && next.closed[..prev.closed.len()] == prev.closed[..]
    {
        return TuiBodyPatch::Incremental {
            append_closed: next.closed[prev.closed.len()..].to_vec(),
            open_plain: next.open_plain.clone(),
            markdown_render: next.markdown_render,
        };
    }
    TuiBodyPatch::ReplaceAll {
        chunks: next.clone(),
    }
}

/// 将助手/用户正文转为可写入 `innerHTML` 的按块流式 HTML。
#[must_use]
#[cfg(test)]
pub fn render_tui_line_markdown(text: &str, finalize_open_line: bool) -> String {
    parse_tui_body_chunks_with(text, finalize_open_line, true).to_inner_html()
}

#[cfg(test)]
fn parse_tui_body_chunks(text: &str, finalize_open_line: bool) -> TuiBodyChunks {
    parse_tui_body_chunks_with(text, finalize_open_line, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_bold_line_renders_strong() {
        let h = render_tui_line_markdown("**你好**\n", false);
        assert!(h.contains("<strong>") || h.contains("<b>"), "got {h}");
        assert!(!h.contains("**你好**"), "got {h}");
    }

    #[test]
    fn incomplete_bold_stays_plain_while_streaming() {
        let h = render_tui_line_markdown("**第一段", false);
        assert!(h.contains("**第一段"), "got {h}");
        assert!(!h.contains("<strong>"), "got {h}");
        assert!(
            h.contains("chat-tui-line--active"),
            "active line class, got {h}"
        );
    }

    #[test]
    fn complete_inline_bold_in_active_line_while_streaming() {
        let h = render_tui_line_markdown("见 **粗体** 与尾", false);
        assert!(h.contains("<strong>"), "got {h}");
        assert!(h.contains("粗体"), "got {h}");
        assert!(!h.contains("**粗体**"), "got {h}");
    }

    #[test]
    fn balanced_then_incomplete_bold_in_active_line() {
        let h = stream_inline_safe_html("**ok** and **no");
        assert!(h.contains("<strong>ok</strong>"), "got {h}");
        assert!(h.contains("**no"), "got {h}");
        assert_eq!(h.matches("<strong>").count(), 1, "got {h}");
    }

    #[test]
    fn active_inline_code_and_escape() {
        let h = stream_inline_safe_html("用 `a<b>` 与 x");
        assert!(h.contains("<code>a&lt;b&gt;</code>"), "got {h}");
    }

    #[test]
    fn finalize_renders_open_line_as_markdown() {
        let h = render_tui_line_markdown("**第一段，第二段**", true);
        assert!(h.contains("<strong>") || h.contains("<b>"), "got {h}");
    }

    #[test]
    fn open_fence_stays_plain_until_closed() {
        let h = render_tui_line_markdown("```rust\nlet x = 1;\n", false);
        assert!(h.contains("let x = 1;"), "got {h}");
        assert!(
            !h.contains("<code"),
            "open fence should stay plain, got {h}"
        );
        assert!(
            h.contains("chat-tui-line--plain"),
            "fence buffer class, got {h}"
        );
    }

    #[test]
    fn closed_fence_renders_code() {
        let h = render_tui_line_markdown("```rust\nlet x = 1;\n```\n", false);
        assert!(h.contains("<code") || h.contains("<pre"), "got {h}");
        assert!(h.contains("let x = 1;"), "got {h}");
    }

    #[test]
    fn closed_prefix_frozen_when_active_grows() {
        let a = parse_tui_body_chunks("done\n\n**a", false);
        let b = parse_tui_body_chunks("done\n\n**ab", false);
        assert_eq!(a.closed, b.closed, "closed chunks must stay identical");
        assert!(!a.closed.is_empty(), "blank should freeze prior paragraph");
        match plan_tui_body_patch(Some(&a), &b) {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
                markdown_render: _,
            } => {
                assert!(append_closed.is_empty());
                assert_eq!(open_plain.as_deref(), Some("**ab"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn streaming_open_line_growth_is_incremental_text_only() {
        let a = parse_tui_body_chunks("**a", false);
        let b = parse_tui_body_chunks("**ab", false);
        match plan_tui_body_patch(Some(&a), &b) {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
                markdown_render: _,
            } => {
                assert!(append_closed.is_empty());
                assert_eq!(open_plain.as_deref(), Some("**ab"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn prose_newline_stays_in_open_until_blank() {
        let a = parse_tui_body_chunks("hello", false);
        let b = parse_tui_body_chunks("hello\nworld", false);
        assert!(a.closed.is_empty());
        assert!(b.closed.is_empty(), "no blank yet, got {:?}", b.closed);
        assert_eq!(b.open_plain.as_deref(), Some("hello\nworld"));
        match plan_tui_body_patch(Some(&a), &b) {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
                markdown_render: _,
            } => {
                assert!(append_closed.is_empty());
                assert_eq!(open_plain.as_deref(), Some("hello\nworld"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn blank_line_freezes_paragraph_block() {
        let a = parse_tui_body_chunks("hello\nworld", false);
        let b = parse_tui_body_chunks("hello\nworld\n\nnext", false);
        match plan_tui_body_patch(Some(&a), &b) {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
                markdown_render: _,
            } => {
                assert!(
                    append_closed.iter().any(|c| c.contains("hello")),
                    "got {append_closed:?}"
                );
                assert_eq!(open_plain.as_deref(), Some("next"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn table_block_freezes_as_one_chunk() {
        let src = "|a|b|\n|---|---|\n|1|2|\n\n";
        let chunks = parse_tui_body_chunks(src, false);
        assert!(
            chunks
                .closed
                .iter()
                .any(|c| c.contains("<table") || c.contains("<th") || c.contains("<td")),
            "got {:?}",
            chunks.closed
        );
        assert!(chunks.open_plain.is_none());
    }

    #[test]
    fn markdown_off_escapes_instead_of_strong() {
        let chunks = parse_tui_body_chunks_with("**x**\n\n", true, false);
        let html = chunks.to_inner_html();
        assert!(!html.contains("<strong>"), "got {html}");
        assert!(
            html.contains("**x**") || html.contains("&#42;"),
            "got {html}"
        );
    }

    #[test]
    fn markdown_off_active_open_line_skips_inline_strong() {
        let h = render_open_active_html("见 **粗体**", false);
        assert!(!h.contains("<strong>"), "got {h}");
        assert!(h.contains("**粗体**") || h.contains("&#42;"), "got {h}");
    }

    #[test]
    fn prose_pipe_is_not_table_block() {
        let chunks = parse_tui_body_chunks("use a | b as or\n\n", false);
        assert!(
            chunks.closed.iter().all(|c| !c.contains("<table")),
            "got {:?}",
            chunks.closed
        );
    }

    #[test]
    fn heading_line_freezes_immediately() {
        let a = parse_tui_body_chunks("# Title\nmore", false);
        assert!(
            a.closed.iter().any(|c| c.contains("Title")),
            "heading should freeze, got {:?}",
            a.closed
        );
        assert_eq!(a.open_plain.as_deref(), Some("more"));
    }
}
