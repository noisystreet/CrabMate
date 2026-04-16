use super::Locale;

// --- 审批条 ---

#[allow(dead_code)]
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

#[allow(dead_code)]
pub fn ellipsis_tail() -> &'static str {
    "…"
}

// --- 审批弹窗 ---

pub fn approval_modal_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "命令审批",
        Locale::En => "Command Approval",
    }
}

pub fn approval_modal_intro(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "即将执行以下命令：",
        Locale::En => "The following command is about to be executed:",
    }
}
