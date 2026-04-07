//! 聊天气泡内 Markdown → 安全 HTML（`ammonia` 白名单），供助手消息渲染。

use pulldown_cmark::{Options, Parser, html};

/// 将 Markdown 转为经净化的 HTML 片段（不含外层 `<html>`）。
pub fn to_safe_html(md: &str) -> String {
    if md.trim().is_empty() {
        return String::new();
    }
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_HEADING_ATTRIBUTES);
    let parser = Parser::new_ext(md, opts);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    ammonia::clean(&body)
}

#[cfg(test)]
mod tests {
    use super::to_safe_html;

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
