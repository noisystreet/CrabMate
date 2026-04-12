use super::Locale;

// --- 工作区树 ---

pub fn workspace_tree_no_data(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（无数据）",
        Locale::En => "(No data)",
    }
}

pub fn workspace_tree_toggle_dir_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开或折叠子目录",
        Locale::En => "Expand or collapse subdirectory",
    }
}

pub fn workspace_tree_expand_folder(l: Locale, name: &str) -> String {
    match l {
        Locale::ZhHans => format!("展开子文件夹 {name}"),
        Locale::En => format!("Expand subfolder {name}"),
    }
}

pub fn workspace_tree_collapse_folder(l: Locale, name: &str) -> String {
    match l {
        Locale::ZhHans => format!("折叠子文件夹 {name}"),
        Locale::En => format!("Collapse subfolder {name}"),
    }
}
