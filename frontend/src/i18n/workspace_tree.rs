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

pub fn workspace_tree_ctx_new_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "新建文件…",
        Locale::En => "New file…",
    }
}

pub fn workspace_tree_ctx_new_dir(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "新建文件夹…",
        Locale::En => "New folder…",
    }
}

pub fn workspace_tree_ctx_delete_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除文件",
        Locale::En => "Delete file",
    }
}

pub fn workspace_tree_ctx_delete_dir(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除文件夹",
        Locale::En => "Delete folder",
    }
}

pub fn workspace_tree_delete_file_confirm(l: Locale, path: &str) -> String {
    match l {
        Locale::ZhHans => format!("确定删除文件 {path}？此操作不可恢复。"),
        Locale::En => format!("Delete file {path}? This cannot be undone."),
    }
}

pub fn workspace_tree_delete_dir_confirm(l: Locale, path: &str, recursive: bool) -> String {
    match l {
        Locale::ZhHans if recursive => {
            format!("确定递归删除文件夹 {path} 及其全部内容？此操作不可恢复。")
        }
        Locale::ZhHans => format!("确定删除空文件夹 {path}？"),
        Locale::En if recursive => {
            format!("Recursively delete folder {path} and all contents? This cannot be undone.")
        }
        Locale::En => format!("Delete empty folder {path}?"),
    }
}

pub fn workspace_tree_name_invalid(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "名称无效：不能包含 /、\\ 或空白",
        Locale::En => "Invalid name: must not contain /, \\, or whitespace",
    }
}

pub fn workspace_tree_inline_name_ph_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "文件名",
        Locale::En => "File name",
    }
}

pub fn workspace_tree_inline_name_ph_dir(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "文件夹名",
        Locale::En => "Folder name",
    }
}
