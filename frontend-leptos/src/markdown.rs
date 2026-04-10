//! 聊天气泡内 Markdown → 安全 HTML（`ammonia` 白名单），供助手消息渲染。

use pulldown_cmark::{Event, Options, Parser, html};

/// 将 Markdown 转为经净化的 HTML 片段（不含外层 `<html>`）。
/// 段落内单换行按硬换行输出 `<br />`，避免在 `white-space: normal` 下被收成空格。
pub fn to_safe_html(md: &str) -> String {
    if md.trim().is_empty() {
        return String::new();
    }
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    let parser = Parser::new_ext(md, opts).map(|e| match e {
        Event::SoftBreak => Event::HardBreak,
        e => e,
    });
    let mut body = String::new();
    html::push_html(&mut body, parser);
    ammonia::clean(&body)
}

/// 调试：不做 Markdown 解析，将纯文本转义为可安全写入 `innerHTML` 的片段（换行 → `<br />`）。
pub fn plaintext_to_safe_html(text: &str) -> String {
    if text.trim().is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len().saturating_mul(2));
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br />"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{plaintext_to_safe_html, to_safe_html};

    #[test]
    fn multi_level_headings_produce_h_tags() {
        let h = to_safe_html("# Title\n\n## Sub\n\n### H3");
        assert!(h.contains("<h1"));
        assert!(h.contains("<h2"));
        assert!(h.contains("<h3"));
    }

    #[test]
    fn empty_or_whitespace_yields_empty() {
        assert!(to_safe_html("").is_empty());
        assert!(to_safe_html("   \n\t  ").is_empty());
    }

    #[test]
    fn table_parsed_and_kept() {
        let h = to_safe_html("|h1|h2|\n|---|---|\n|a|b|");
        assert!(h.contains("<table"));
        assert!(h.to_lowercase().contains("h1"));
    }

    #[test]
    fn script_tag_stripped_by_ammonia() {
        let h = to_safe_html("Hello<script>alert(1)</script>");
        assert!(!h.to_lowercase().contains("<script"));
        assert!(h.contains("Hello"));
    }

    #[test]
    fn fenced_code_emits_pre_or_code() {
        let h = to_safe_html("```rust\nlet x = 1;\n```");
        assert!(h.contains("<pre") || h.contains("<code"));
    }

    #[test]
    fn single_newline_in_paragraph_emits_line_break() {
        let h = to_safe_html("不调用任何工具\n用 JSON 回复");
        let lower = h.to_lowercase();
        assert!(
            lower.contains("<br") || lower.contains("br>"),
            "expected hard line break in HTML, got {h:?}"
        );
    }

    #[test]
    fn plaintext_escapes_and_line_breaks() {
        let h = plaintext_to_safe_html("a <b>\nc");
        assert!(h.contains("&lt;"));
        assert!(h.to_lowercase().contains("<br"));
        assert!(!h.contains("<b>"));
    }
}

/// WASM 下由 `wasm-bindgen-test` 跑通「Markdown → 净化 HTML」链路（与 CSR 目标一致）。
#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_bindgen_tests {
    use wasm_bindgen_test::wasm_bindgen_test;

    use super::to_safe_html;

    #[wasm_bindgen_test]
    fn wasm_markdown_bold_and_sanitized() {
        let h = to_safe_html("**x**");
        assert!(
            h.contains("<strong>") || h.contains("<b>"),
            "expected bold tag, got {h:?}"
        );
    }

    #[wasm_bindgen_test]
    fn wasm_markdown_table() {
        let h = to_safe_html("|c|\n|-|\n|v|");
        assert!(h.contains("<table"), "expected table, got {h:?}");
    }
}
