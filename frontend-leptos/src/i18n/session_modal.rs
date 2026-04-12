use super::Locale;

// --- 会话列表模态 ---

pub fn session_modal_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话",
        Locale::En => "Sessions",
    }
}

pub fn session_modal_badge(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本地",
        Locale::En => "Local",
    }
}

pub fn session_modal_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "本地保存在浏览器；可导出为与 CLI save-session 同形的 JSON / Markdown 下载。"
        }
        Locale::En => "Stored in the browser; export as JSON / Markdown matching CLI save-session.",
    }
}

pub fn session_row_msg_count(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 条"),
        Locale::En => {
            if n == 1 {
                "1 message".to_string()
            } else {
                format!("{n} messages")
            }
        }
    }
}

pub fn session_row_rename_title_attr(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重命名",
        Locale::En => "Rename",
    }
}

pub fn session_row_rename_button(l: Locale) -> &'static str {
    session_row_rename_title_attr(l)
}

pub fn session_prompt_title_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话标题",
        Locale::En => "Session title",
    }
}

pub fn session_row_export_json_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出 JSON（ChatSessionFile v1）",
        Locale::En => "Export JSON (ChatSessionFile v1)",
    }
}

pub fn session_row_export_md_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "导出 Markdown",
        Locale::En => "Export Markdown",
    }
}

pub fn session_row_delete_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除此会话",
        Locale::En => "Delete this session",
    }
}

pub fn session_row_delete_button(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除",
        Locale::En => "Delete",
    }
}
