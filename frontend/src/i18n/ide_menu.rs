use super::Locale;

pub fn ide_menu_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "文件",
        Locale::En => "File",
    }
}

pub fn ide_menu_edit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑",
        Locale::En => "Edit",
    }
}

pub fn ide_menu_view(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "视图",
        Locale::En => "View",
    }
}

pub fn ide_menu_save(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存 (Ctrl+S)",
        Locale::En => "Save (Ctrl+S)",
    }
}

pub fn ide_menu_save_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "全部保存 (Ctrl+Shift+S)",
        Locale::En => "Save all (Ctrl+Shift+S)",
    }
}

pub fn ide_menu_new_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "新建文件…",
        Locale::En => "New file…",
    }
}

pub fn ide_new_file_prompt(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "相对工作区的文件路径（例如 src/main.rs）",
        Locale::En => "Workspace-relative file path (e.g. src/main.rs)",
    }
}

pub fn ide_disk_reload_confirm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "磁盘上的文件已被外部修改。重载将丢弃当前未保存的编辑，是否继续？",
        Locale::En => "The file changed on disk. Reload and discard unsaved edits?",
    }
}

pub fn ide_menu_back_to_chat(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "返回对话布局",
        Locale::En => "Back to chat layout",
    }
}

pub fn ide_menu_select_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "全选",
        Locale::En => "Select all",
    }
}

pub fn ide_menu_editor_settings(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑器设置…",
        Locale::En => "Editor settings…",
    }
}

pub fn ide_menu_toggle_line_numbers(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "显示行号",
        Locale::En => "Show line numbers",
    }
}

pub fn ide_menu_toggle_word_wrap(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "自动换行",
        Locale::En => "Word wrap",
    }
}

pub fn ide_menu_bar_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑器菜单",
        Locale::En => "Editor menu bar",
    }
}

pub fn ide_tauri_window_controls_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "窗口控制",
        Locale::En => "Window controls",
    }
}

pub fn ide_tauri_window_minimize(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "最小化",
        Locale::En => "Minimize",
    }
}

pub fn ide_tauri_window_toggle_maximize(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "最大化或还原",
        Locale::En => "Maximize or restore",
    }
}

pub fn ide_tauri_window_close(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭",
        Locale::En => "Close",
    }
}
