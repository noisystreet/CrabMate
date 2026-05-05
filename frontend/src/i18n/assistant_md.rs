use super::Locale;

// --- 助手 Markdown 折叠 ---

pub fn assistant_md_collapse(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起",
        Locale::En => "Collapse",
    }
}

pub fn assistant_md_expand_full(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开全文",
        Locale::En => "Expand full text",
    }
}
