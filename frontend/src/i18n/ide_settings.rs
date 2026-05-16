use super::Locale;

pub fn ide_settings_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑器设置",
        Locale::En => "Editor settings",
    }
}

pub fn ide_settings_badge_local(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本机",
        Locale::En => "Local",
    }
}

pub fn ide_settings_back(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "返回编辑器",
        Locale::En => "Back to editor",
    }
}

pub fn ide_settings_nav_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑器设置分区",
        Locale::En => "Editor settings sections",
    }
}

pub fn ide_settings_section_editor_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑器",
        Locale::En => "Editor",
    }
}

pub fn ide_settings_section_editor_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "字体、行号与换行等仅保存在本浏览器。",
        Locale::En => "Font, line numbers, and wrap are stored in this browser only.",
    }
}

pub fn ide_settings_block_font(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "字体",
        Locale::En => "Font",
    }
}

pub fn ide_settings_label_font_family(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "字体族",
        Locale::En => "Font family",
    }
}

pub fn ide_settings_font_jetbrains(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "JetBrains Mono",
        Locale::En => "JetBrains Mono",
    }
}

pub fn ide_settings_font_cascadia(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Cascadia Code",
        Locale::En => "Cascadia Code",
    }
}

pub fn ide_settings_font_fira(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Fira Code",
        Locale::En => "Fira Code",
    }
}

pub fn ide_settings_font_system(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "系统等宽",
        Locale::En => "System monospace",
    }
}

pub fn ide_settings_font_label(l: Locale, slug: &str) -> &'static str {
    match slug {
        "cascadia" => ide_settings_font_cascadia(l),
        "fira" => ide_settings_font_fira(l),
        "system" => ide_settings_font_system(l),
        _ => ide_settings_font_jetbrains(l),
    }
}

pub fn ide_settings_label_font_size(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "字号（px）",
        Locale::En => "Font size (px)",
    }
}

pub fn ide_settings_block_display(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示",
        Locale::En => "Display",
    }
}

pub fn ide_settings_line_numbers(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示行号",
        Locale::En => "Show line numbers",
    }
}

pub fn ide_settings_word_wrap(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "自动换行",
        Locale::En => "Word wrap",
    }
}

pub fn ide_settings_label_tab_size(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Tab 宽度（空格）",
        Locale::En => "Tab width (spaces)",
    }
}

pub fn ide_settings_unsaved_badge(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "未保存",
        Locale::En => "Unsaved",
    }
}

pub fn ide_settings_discard_changes(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "放弃更改",
        Locale::En => "Discard",
    }
}

pub fn ide_settings_save_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存",
        Locale::En => "Save",
    }
}
