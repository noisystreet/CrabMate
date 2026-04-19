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

// --- 思考区 ---

pub fn assistant_thinking_section_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "思考过程",
        Locale::En => "Thinking",
    }
}

pub fn assistant_thinking_collapse(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起思考",
        Locale::En => "Hide thinking",
    }
}

pub fn assistant_thinking_expand(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开思考",
        Locale::En => "Show thinking",
    }
}
