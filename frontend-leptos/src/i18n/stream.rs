use super::Locale;

// --- 流式 / 停止 ---

pub fn stream_empty_reply(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复)",
        Locale::En => "(No reply)",
    }
}

pub fn stream_empty_reply_no_answer_phase(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复：未进入正文阶段)",
        Locale::En => "(No reply: answer phase not entered)",
    }
}

pub fn stream_empty_reply_no_delta(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(无回复：未收到正文片段)",
        Locale::En => "(No reply: no answer delta received)",
    }
}

pub fn stream_empty_reply_diag_line(
    l: Locale,
    stream_end_reason: Option<&str>,
    answer_phase: bool,
    delta_chars: usize,
) -> String {
    let reason = stream_end_reason.unwrap_or("unknown");
    match l {
        Locale::ZhHans => format!(
            "诊断：stream_ended={reason}, answer_phase={answer_phase}, delta_chars={delta_chars}"
        ),
        Locale::En => format!(
            "Diagnostic: stream_ended={reason}, answer_phase={answer_phase}, delta_chars={delta_chars}"
        ),
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
