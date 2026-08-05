//! 窄屏 Web 壳层文案。

use super::Locale;

pub fn mobile_tab_chat(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "聊天",
        Locale::En => "Chat",
    }
}

pub fn mobile_tab_workspace(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工作区",
        Locale::En => "Workspace",
    }
}

pub fn mobile_tab_tasks(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "任务",
        Locale::En => "Tasks",
    }
}

pub fn mobile_tab_more(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "更多",
        Locale::En => "More",
    }
}

pub fn mobile_tab_bar_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "主界面导航",
        Locale::En => "Main navigation",
    }
}

pub fn mobile_side_sheet_close(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭面板",
        Locale::En => "Close panel",
    }
}

pub fn mobile_status_overflow_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "更多状态信息",
        Locale::En => "More status details",
    }
}

pub fn mobile_status_overflow_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "状态详情",
        Locale::En => "Status details",
    }
}
