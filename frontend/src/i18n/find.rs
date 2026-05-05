use super::Locale;

// --- 查找栏 ---

pub fn chat_find_region(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在当前会话中查找",
        Locale::En => "Find in this conversation",
    }
}

pub fn chat_find_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查找",
        Locale::En => "Find",
    }
}

pub fn chat_find_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前会话消息…",
        Locale::En => "Messages in this chat…",
    }
}

pub fn chat_find_no_match(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无匹配",
        Locale::En => "No match",
    }
}

pub fn chat_find_prev_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上一条匹配",
        Locale::En => "Previous match",
    }
}

pub fn chat_find_next_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "下一条匹配",
        Locale::En => "Next match",
    }
}

pub fn chat_find_close_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起查找栏",
        Locale::En => "Close find bar",
    }
}

pub fn chat_find_close_aria(l: Locale) -> &'static str {
    chat_find_close_title(l)
}
