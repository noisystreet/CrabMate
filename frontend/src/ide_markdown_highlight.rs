//! 轻量 Markdown 语法高亮（标题、围栏代码、行内代码、粗体）。

use crate::ide_highlight_common::push_span;

#[must_use]
pub fn highlight_markdown_to_html(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + source.len() / 4);
    let mut in_fence = false;
    for line in source.split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n');
        if trimmed.starts_with("```") {
            in_fence = !in_fence;
            push_span(&mut out, if in_fence { "hl-kw" } else { "hl-plain" }, line);
            continue;
        }
        if in_fence {
            push_span(&mut out, "hl-plain", line);
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('#') {
            let hashes = rest.chars().take_while(|c| *c == '#').count();
            let heading = format!("{}{}", "#".repeat(hashes), &rest[hashes..]);
            push_span(&mut out, "hl-kw", &heading);
            if line.ends_with('\n') {
                out.push('\n');
            }
            continue;
        }
        highlight_markdown_inline_line(&mut out, line);
    }
    out
}

fn highlight_markdown_inline_line(out: &mut String, line: &str) {
    let mut i = 0usize;
    let chars: Vec<char> = line.chars().collect();
    let mut buf = String::new();
    let flush = |out: &mut String, class: &str, buf: &mut String| {
        if buf.is_empty() {
            return;
        }
        push_span(out, class, buf);
        buf.clear();
    };
    while i < chars.len() {
        if chars[i] == '`' {
            flush(out, "hl-plain", &mut buf);
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '`' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            let seg: String = chars[start..i].iter().collect();
            push_span(out, "hl-str", &seg);
            continue;
        }
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            flush(out, "hl-plain", &mut buf);
            i += 2;
            let start = i;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '*') {
                i += 1;
            }
            let inner: String = chars[start..i].iter().collect();
            push_span(out, "hl-kw", &format!("**{inner}**"));
            if i + 1 < chars.len() {
                i += 2;
            }
            continue;
        }
        buf.push(chars[i]);
        i += 1;
    }
    flush(out, "hl-plain", &mut buf);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md_heading_smoke() {
        assert!(highlight_markdown_to_html("# Title\n").contains("hl-kw"));
    }
}
