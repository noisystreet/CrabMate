//! 终端流按行 Markdown：已闭合行做安全 HTML（冻结，增量只 append）；
//! 活跃末行做**流式安全**行内增强（成对 `**` / `` ` `` / `~~` 才着色；半截标记保持字面量）；
//! 未闭合围栏仍纯文本。提供 chunk 解析与增量 patch，供按回合局部 DOM 更新。

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
/// 不做标题/列表/围栏（那些等换行进 closed 块后再 `to_safe_html`）。
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
                // 粗体内允许嵌套行内 code；其余转义
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

/// 活跃行写入 DOM / `to_inner_html`：围栏缓冲纯文本，否则流式行内 HTML。
#[must_use]
pub fn render_open_active_html(text: &str) -> String {
    if open_line_is_fence_buffer(text) {
        plaintext_to_safe_html(text)
    } else {
        stream_inline_safe_html(text)
    }
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

fn md_line_html(line: &str) -> String {
    if line.is_empty() {
        return "<div class=\"chat-tui-line chat-tui-line--blank\"><br /></div>".to_string();
    }
    let html = to_safe_html(line);
    if html.is_empty() {
        "<div class=\"chat-tui-line chat-tui-line--blank\"><br /></div>".to_string()
    } else {
        format!("<div class=\"chat-tui-line\">{html}</div>")
    }
}

fn fence_html(lang: &str, body: &str) -> String {
    let mut fenced = String::with_capacity(body.len() + lang.len() + 16);
    fenced.push_str("```");
    fenced.push_str(lang);
    fenced.push('\n');
    fenced.push_str(body);
    if !body.is_empty() && !body.ends_with('\n') {
        fenced.push('\n');
    }
    fenced.push_str("```");
    let html = to_safe_html(&fenced);
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

/// 按行解析结果：闭合块 HTML 列表（冻结）+ 可选活跃行**源文本**（渲染见 [`render_open_active_html`]）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TuiBodyChunks {
    pub closed: Vec<String>,
    /// 活跃行源文本；围栏缓冲以 \`\`\` 开头。DOM 用 [`render_open_active_html`] 写入。
    pub open_plain: Option<String>,
}

fn open_active_line_class(text: &str) -> &'static str {
    if open_line_is_fence_buffer(text) {
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
            out.push_str(open_active_line_class(plain));
            out.push_str("\">");
            out.push_str(&render_open_active_html(plain));
            out.push_str("</div>");
        }
        out
    }
}

/// live body 增量：优先 append **冻结**闭合行 + 只改活跃行；工具行改 status/one-line；否则整 body 替换。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TuiBodyPatch {
    ReplaceAll {
        chunks: TuiBodyChunks,
    },
    Incremental {
        append_closed: Vec<String>,
        /// 活跃行源文本（非 HTML）；`None` 表示移除活跃行节点。
        open_plain: Option<String>,
    },
    /// 工具折叠行：只改文案，不重写 HTML（高度与结构保持不变）。
    ToolRow {
        status: String,
        one_line: String,
        /// 若 DOM 已有 details，同步更新详情正文；`None` 表示无详情块。
        detail: Option<String>,
    },
}

/// 将正文解析为可挂载的 closed / open 块。
#[must_use]
pub fn parse_tui_body_chunks(text: &str, finalize_open_line: bool) -> TuiBodyChunks {
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

    let mut closed = Vec::new();
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_body = String::new();

    for line in complete {
        if is_fence_marker(line) {
            if in_fence {
                closed.push(fence_html(&fence_lang, &fence_body));
                in_fence = false;
                fence_lang.clear();
                fence_body.clear();
            } else {
                in_fence = true;
                fence_lang = fence_info(line).to_string();
                fence_body.clear();
            }
            continue;
        }
        if in_fence {
            fence_body.push_str(line);
            fence_body.push('\n');
        } else {
            closed.push(md_line_html(line));
        }
    }

    let open_plain = match open_line {
        Some(open) if in_fence => Some(open_fence_plain_text(&fence_lang, &fence_body, open)),
        Some(open) if finalize_open_line => {
            closed.push(md_line_html(open));
            None
        }
        Some(open) => Some(open.to_string()),
        None if in_fence => Some(open_fence_plain_text(&fence_lang, &fence_body, "")),
        None => None,
    };

    TuiBodyChunks { closed, open_plain }
}

/// 对比上一帧 closed 前缀：可增量则 append，否则整段替换。
#[must_use]
pub fn plan_tui_body_patch(prev: Option<&TuiBodyChunks>, next: &TuiBodyChunks) -> TuiBodyPatch {
    let Some(prev) = prev else {
        return TuiBodyPatch::ReplaceAll {
            chunks: next.clone(),
        };
    };
    if next.closed.len() >= prev.closed.len() && next.closed[..prev.closed.len()] == prev.closed[..]
    {
        return TuiBodyPatch::Incremental {
            append_closed: next.closed[prev.closed.len()..].to_vec(),
            open_plain: next.open_plain.clone(),
        };
    }
    TuiBodyPatch::ReplaceAll {
        chunks: next.clone(),
    }
}

/// 将助手/用户正文转为可写入 `innerHTML` 的按行流式 HTML。
#[must_use]
#[cfg(test)]
pub fn render_tui_line_markdown(text: &str, finalize_open_line: bool) -> String {
    parse_tui_body_chunks(text, finalize_open_line).to_inner_html()
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
        let a = parse_tui_body_chunks("done\n**a", false);
        let b = parse_tui_body_chunks("done\n**ab", false);
        assert_eq!(a.closed, b.closed, "closed chunks must stay identical");
        match plan_tui_body_patch(Some(&a), &b) {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
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
            } => {
                assert!(append_closed.is_empty());
                assert_eq!(open_plain.as_deref(), Some("**ab"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }

    #[test]
    fn newline_promotes_open_to_closed_chunk() {
        let a = parse_tui_body_chunks("hello", false);
        let b = parse_tui_body_chunks("hello\nworld", false);
        match plan_tui_body_patch(Some(&a), &b) {
            TuiBodyPatch::Incremental {
                append_closed,
                open_plain,
            } => {
                assert_eq!(append_closed.len(), 1);
                assert!(
                    append_closed[0].contains("hello"),
                    "got {}",
                    append_closed[0]
                );
                assert_eq!(open_plain.as_deref(), Some("world"));
            }
            other => panic!("expected Incremental, got {other:?}"),
        }
    }
}
