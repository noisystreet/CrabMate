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

pub fn ws_path_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "未选择工作区",
        Locale::En => "No workspace selected",
    }
}

pub fn ws_path_title_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "用「项目 → 选择工作区」、项目列表或最近列表设置",
        Locale::En => "Set via Project → Open workspace, project list, or Recent",
    }
}

pub fn ws_project_modal_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选择或新建项目",
        Locale::En => "Open or create project",
    }
}

pub fn ws_project_modal_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "项目目录位于服务器项目池内；名称仅可含字母、数字及 . _ -",
        Locale::En => "Projects live in the server pool; names may use letters, digits, and . _ -",
    }
}

pub fn ws_project_new_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "新建项目",
        Locale::En => "New project",
    }
}

pub fn ws_project_new_placeholder(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "例如 my-app",
        Locale::En => "e.g. my-app",
    }
}

pub fn ws_project_create_open(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "创建并打开",
        Locale::En => "Create & open",
    }
}

pub fn ws_project_open(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "打开",
        Locale::En => "Open",
    }
}

pub fn ws_project_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "尚无项目，请在下方输入名称创建。",
        Locale::En => "No projects yet. Enter a name below to create one.",
    }
}

pub fn ws_project_loading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在加载项目列表…",
        Locale::En => "Loading projects…",
    }
}

/// 浏览器（非 Tauri）下手输绝对路径的输入框标签（原 `window.prompt` 文案）。
pub fn ws_path_prompt(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "服务器上的工作区绝对路径",
        Locale::En => "Absolute workspace path on the server",
    }
}

pub fn ws_browser_pick_modal_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "选择工作区",
        Locale::En => "Choose workspace",
    }
}

pub fn ws_browser_pick_modal_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "手机浏览器无法打开系统目录对话框。请从最近列表选择，或输入远程服务器上的绝对路径。"
        }
        Locale::En => {
            "Mobile browsers cannot open a folder picker. Pick a recent root, or enter an absolute path on the remote server."
        }
    }
}

pub fn ws_browser_pick_recent_heading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "最近打开",
        Locale::En => "Recent",
    }
}

pub fn ws_browser_pick_recent_empty(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "暂无最近工作区。",
        Locale::En => "No recent workspaces yet.",
    }
}

pub fn ws_browser_pick_path_heading(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "输入路径",
        Locale::En => "Enter path",
    }
}

pub fn ws_browser_pick_submit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "打开",
        Locale::En => "Open",
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
