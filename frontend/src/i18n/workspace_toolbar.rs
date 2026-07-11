use super::Locale;
use super::changelist_loading;
use super::changelist_refresh;

// --- 侧栏工具栏 / 工作区 ---

pub fn side_resize_handle(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "拖拽调整右列宽度",
        Locale::En => "Drag to resize right column",
    }
}

pub fn side_toolbar_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "GitHub 仓库、视图与设置",
        Locale::En => "GitHub repository, view and settings",
    }
}

pub fn side_view_menu_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选择侧栏：隐藏 / 工作区 / 任务 / 调试台",
        Locale::En => "Side panel: hide / workspace / tasks / debug console",
    }
}

pub fn side_view_menu_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "侧栏视图",
        Locale::En => "Side panel view",
    }
}

pub fn side_panel_hide(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "隐藏侧栏",
        Locale::En => "Hide panel",
    }
}

pub fn side_panel_workspace(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工作区",
        Locale::En => "Workspace",
    }
}

pub fn side_panel_tasks(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "任务",
        Locale::En => "Tasks",
    }
}

pub fn side_github_repo_btn_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "打开 GitHub 仓库",
        Locale::En => "Open GitHub repository",
    }
}

pub fn side_github_repo_btn_aria(l: Locale, repo: &str) -> String {
    match l {
        Locale::ZhHans => format!("在应用内打开 GitHub 仓库 {repo}"),
        Locale::En => format!("Open GitHub repository {repo} in app"),
    }
}

pub fn side_status_btn_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "状态栏",
        Locale::En => "Status bar",
    }
}

pub fn side_settings_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "外观与背景",
        Locale::En => "Appearance",
    }
}

pub fn side_debug_console_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开或收起思维与工具调试台",
        Locale::En => "Show or hide thinking / tool debug console",
    }
}

pub fn side_debug_console_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "调试台",
        Locale::En => "Debug",
    }
}

pub fn tasks_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "任务清单",
        Locale::En => "Tasks",
    }
}

pub fn tasks_loading(l: Locale) -> &'static str {
    changelist_loading(l)
}

pub fn tasks_error(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "错误",
        Locale::En => "Error",
    }
}

pub fn tasks_done_ratio(l: Locale, done: usize, total: usize) -> String {
    match l {
        Locale::ZhHans => format!("{done}/{total} 完成"),
        Locale::En => format!("{done}/{total} done"),
    }
}

pub fn tasks_refresh(l: Locale) -> &'static str {
    changelist_refresh(l)
}

pub fn tasks_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载任务",
        Locale::En => "Loading tasks",
    }
}

pub fn ws_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载工作区",
        Locale::En => "Loading workspace",
    }
}

pub fn ws_root_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工作区根目录",
        Locale::En => "Workspace root",
    }
}

pub fn ws_input_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "绝对路径（允许根内）；桌面壳可点「浏览」选目录并提交；浏览器请手输后按 Enter"
        }
        Locale::En => {
            "Absolute path (within allowed roots); desktop app can Browse and submit; browser: type and Enter"
        }
    }
}

pub fn ws_input_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "桌面壳（Tauri）可用系统文件夹对话框；浏览器仅手输路径后按 Enter",
        Locale::En => {
            "Desktop (Tauri) can use the native folder picker; browser: type path and press Enter"
        }
    }
}

pub fn ws_browse_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "浏览…",
        Locale::En => "Browse…",
    }
}

pub fn ws_browse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选择工作区根目录",
        Locale::En => "Pick workspace root folder",
    }
}

pub fn ws_browse_busy_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在打开文件夹对话框…",
        Locale::En => "Opening folder picker…",
    }
}

pub fn ws_path_required(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请填写目录路径。",
        Locale::En => "Please enter a directory path.",
    }
}

pub fn ws_refresh_list(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "刷新列表",
        Locale::En => "Refresh list",
    }
}

pub fn ws_changelog_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "查看本会话工具写入的 unified diff 摘要（与注入模型的变更集同源）",
        Locale::En => "View unified diff summary for this session (same as model changelist)",
    }
}

pub fn ws_changelog_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "变更预览",
        Locale::En => "Change preview",
    }
}
