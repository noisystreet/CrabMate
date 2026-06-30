//! IDE 编辑器查找、跳转、确认与新建文件文案。

use super::Locale;

pub fn ide_find_region(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在文件中查找",
        Locale::En => "Find in file",
    }
}

pub fn ide_find_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查找",
        Locale::En => "Find",
    }
}

pub fn ide_find_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "搜索当前文件…",
        Locale::En => "Search in file…",
    }
}

pub fn ide_find_no_match(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "无匹配",
        Locale::En => "No matches",
    }
}

pub fn ide_find_prev_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上一处",
        Locale::En => "Previous match",
    }
}

pub fn ide_find_next_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "下一处",
        Locale::En => "Next match",
    }
}

pub fn ide_find_close_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭查找",
        Locale::En => "Close find",
    }
}

pub fn ide_find_close_aria(l: Locale) -> &'static str {
    ide_find_close_title(l)
}

pub fn ide_goto_region(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "跳转到行",
        Locale::En => "Go to line",
    }
}

pub fn ide_goto_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "行号",
        Locale::En => "Line",
    }
}

pub fn ide_goto_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "输入行号后按 Enter",
        Locale::En => "Line number, press Enter",
    }
}

pub fn ide_goto_close_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭跳转",
        Locale::En => "Close go to line",
    }
}

pub fn ide_goto_close_aria(l: Locale) -> &'static str {
    ide_goto_close_title(l)
}

pub fn ide_confirm_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "确认",
        Locale::En => "Confirm",
    }
}

pub fn ide_confirm_ok(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "放弃并继续",
        Locale::En => "Discard and continue",
    }
}

pub fn ide_confirm_cancel(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "取消",
        Locale::En => "Cancel",
    }
}

pub fn ide_new_file_placeholder(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "例如 src/main.rs",
        Locale::En => "e.g. src/main.rs",
    }
}

pub fn ide_new_file_cancel(l: Locale) -> &'static str {
    ide_confirm_cancel(l)
}

pub fn ide_new_file_create(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "创建",
        Locale::En => "Create",
    }
}

pub fn ide_menu_find(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查找 (Ctrl+F)",
        Locale::En => "Find (Ctrl+F)",
    }
}

pub fn ide_menu_goto_line(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "跳转到行 (Ctrl+G)",
        Locale::En => "Go to line (Ctrl+G)",
    }
}
