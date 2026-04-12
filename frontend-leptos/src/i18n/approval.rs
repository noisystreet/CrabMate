use super::Locale;

// --- 审批条 ---

pub fn approval_toggle_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "需要审批：运行命令",
        Locale::En => "Approval required: run command",
    }
}

pub fn approval_deny(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "拒绝",
        Locale::En => "Deny",
    }
}

pub fn approval_allow_once(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "允许一次",
        Locale::En => "Allow once",
    }
}

pub fn approval_allow_always(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "始终允许",
        Locale::En => "Always allow",
    }
}

pub fn ellipsis_tail() -> &'static str {
    "…"
}
