use super::Locale;

// --- 变更集模态 ---

pub fn changelist_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "会话工作区变更",
        Locale::En => "Workspace changes (session)",
    }
}

pub fn changelist_refresh(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "刷新",
        Locale::En => "Refresh",
    }
}

pub fn changelist_loading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载中…",
        Locale::En => "Loading…",
    }
}

pub fn changelist_rev(l: Locale, n: u64) -> String {
    match l {
        Locale::ZhHans => format!("rev {n}"),
        Locale::En => format!("rev {n}"),
    }
}
