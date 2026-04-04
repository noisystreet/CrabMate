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
}
