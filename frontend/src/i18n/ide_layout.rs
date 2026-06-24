//! Web IDE 布局（工作区树 + 文本编辑器）文案。

use super::Locale;

pub fn ide_toggle_editor(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "编辑器",
        Locale::En => "Editor",
    }
}

pub fn ide_toggle_chat(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "对话",
        Locale::En => "Chat",
    }
}

pub fn ide_toggle_editor_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "切换到编辑器布局",
        Locale::En => "Switch to editor layout",
    }
}

pub fn ide_toggle_chat_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "切换到对话布局",
        Locale::En => "Switch to chat layout",
    }
}

pub fn ide_workspace_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工作区",
        Locale::En => "Workspace",
    }
}

pub fn ide_open_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "单击文件在新标签页打开；双击将 @相对路径 插入对话输入框并切回对话布局。",
        Locale::En => {
            "Click to open a tab; double-click inserts @path into chat and switches to chat layout."
        }
    }
}

pub fn ide_tabs_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已打开文件",
        Locale::En => "Open files",
    }
}

pub fn ide_tab_close_aria(l: Locale, name: &str) -> String {
    match l {
        Locale::ZhHans => format!("关闭 {name}"),
        Locale::En => format!("Close {name}"),
    }
}

pub fn ide_no_file(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请从左侧选择文件。",
        Locale::En => "Pick a file from the tree.",
    }
}

pub fn ide_saving(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "保存中…",
        Locale::En => "Saving…",
    }
}

pub fn ide_dirty_confirm(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "当前文件有未保存更改，放弃并继续？",
        Locale::En => "Discard unsaved changes and continue?",
    }
}
