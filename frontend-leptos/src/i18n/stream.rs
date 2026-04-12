use super::Locale;

// --- 流式 / 停止 ---

pub fn stream_empty_reply(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复)",
        Locale::En => "(No reply)",
    }
}

pub fn stream_stopped_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "\n\n[已停止]",
        Locale::En => "\n\n[Stopped]",
    }
}

pub fn stream_stopped_inline(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已停止",
        Locale::En => "Stopped",
    }
}

pub fn chat_failed_banner(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "对话失败",
        Locale::En => "Chat failed",
    }
}
