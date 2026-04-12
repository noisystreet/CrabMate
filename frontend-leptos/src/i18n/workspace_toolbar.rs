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
        Locale::ZhHans => "视图与设置",
        Locale::En => "View and settings",
    }
}

pub fn side_view_menu_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选择侧栏：隐藏 / 工作区 / 任务",
        Locale::En => "Side panel: hide / workspace / tasks",
    }
}

pub fn side_view_label(l: Locale, open: bool) -> String {
    match l {
        Locale::ZhHans => {
            let s = if open { "▴" } else { "▾" };
            format!("视图{s}")
        }
        Locale::En => {
            let s = if open { "▴" } else { "▾" };
            format!("View{s}")
        }
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

pub fn side_status_btn_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "状态栏",
        Locale::En => "Status bar",
    }
}

pub fn side_status_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "状态",
        Locale::En => "Status",
    }
}

pub fn side_settings_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "外观与背景",
        Locale::En => "Appearance",
    }
}

pub fn side_settings_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "设置",
        Locale::En => "Settings",
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
        Locale::ZhHans => "绝对路径（允许根内）；浏览选目录将自动生效，手动输入后按 Enter",
        Locale::En => {
            "Absolute path (within allowed roots); pick applies automatically, or type and press Enter"
        }
    }
}

pub fn ws_input_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在运行 serve 的机器上选目录后会立即提交；亦可手输路径后按 Enter",
        Locale::En => {
            "Picking a folder on the serve host submits immediately; or type a path and press Enter"
        }
    }
}

pub fn ws_path_required(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请填写目录路径。",
        Locale::En => "Please enter a directory path.",
    }
}

pub fn ws_browse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在运行 serve 的机器上打开系统选目录对话框",
        Locale::En => "Open folder picker on the serve host",
    }
}

pub fn ws_pick_none(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "未选择目录，或服务端无法弹窗（无图形/无头/SSH 远端）。请手动填写路径后按 Enter。"
        }
        Locale::En => {
            "No folder chosen, or the server cannot show a dialog (headless/SSH). Enter a path manually and press Enter."
        }
    }
}

pub fn ws_browse_busy(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "…",
        Locale::En => "…",
    }
}

pub fn ws_browse_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "浏览…",
        Locale::En => "Browse…",
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
