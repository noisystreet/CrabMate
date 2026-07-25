//! 终端流按行 Markdown：已闭合行（含 `\n`）做安全 HTML；未闭合末行流式期保持纯文本；
//! 围栏代码块开闭完整后再整块渲染，未闭合围栏整段纯文本。

use crate::markdown::{plaintext_to_safe_html, to_safe_html};

fn is_fence_marker(line: &str) -> bool {
    line.trim_start().starts_with("```")
}

fn fence_info(line: &str) -> &str {
    line.trim_start()
        .strip_prefix("```")
        .map(str::trim)
        .unwrap_or("")
}

fn push_md_line(line: &str, out: &mut String) {
    if line.is_empty() {
        out.push_str("<div class=\"chat-tui-line chat-tui-line--blank\"><br /></div>");
        return;
    }
    let html = to_safe_html(line);
    if html.is_empty() {
        out.push_str("<div class=\"chat-tui-line chat-tui-line--blank\"><br /></div>");
    } else {
        out.push_str("<div class=\"chat-tui-line\">");
        out.push_str(&html);
        out.push_str("</div>");
    }
}

fn push_plain_fragment(text: &str, out: &mut String) {
    if text.is_empty() {
        return;
    }
    out.push_str("<div class=\"chat-tui-line chat-tui-line--plain\">");
    out.push_str(&plaintext_to_safe_html(text));
    out.push_str("</div>");
}

fn flush_closed_fence(lang: &str, body: &str, out: &mut String) {
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
    out.push_str("<div class=\"chat-tui-line chat-tui-line--fence\">");
    out.push_str(&html);
    out.push_str("</div>");
}

fn push_open_fence_plain(lang: &str, body: &str, open_tail: &str, out: &mut String) {
    let mut plain = String::new();
    plain.push_str("```");
    plain.push_str(lang);
    plain.push('\n');
    plain.push_str(body);
    plain.push_str(open_tail);
    push_plain_fragment(&plain, out);
}

/// 将助手/用户正文转为可写入 `innerHTML` 的按行流式 HTML。
///
/// - `finalize_open_line == false`：末尾无 `\n` 的半行保持转义纯文本（流式中）。
/// - `finalize_open_line == true`：末行也按 Markdown 渲染（消息已落定）。
#[must_use]
pub fn render_tui_line_markdown(text: &str, finalize_open_line: bool) -> String {
    if text.is_empty() {
        return String::new();
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

    let mut out = String::with_capacity(text.len().saturating_mul(2));
    let mut in_fence = false;
    let mut fence_lang = String::new();
    let mut fence_body = String::new();

    for line in complete {
        if is_fence_marker(line) {
            if in_fence {
                flush_closed_fence(&fence_lang, &fence_body, &mut out);
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
            push_md_line(line, &mut out);
        }
    }

    match open_line {
        Some(open) if in_fence => {
            push_open_fence_plain(&fence_lang, &fence_body, open, &mut out);
        }
        Some(open) if finalize_open_line => {
            push_md_line(open, &mut out);
        }
        Some(open) => {
            push_plain_fragment(open, &mut out);
        }
        None if in_fence => {
            push_open_fence_plain(&fence_lang, &fence_body, "", &mut out);
        }
        None => {}
    }

    out
}

#[cfg(test)]
mod tests {
    use super::render_tui_line_markdown;

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
    }

    #[test]
    fn closed_fence_renders_code() {
        let h = render_tui_line_markdown("```rust\nlet x = 1;\n```\n", false);
        assert!(h.contains("<code") || h.contains("<pre"), "got {h}");
        assert!(h.contains("let x = 1;"), "got {h}");
    }
}
