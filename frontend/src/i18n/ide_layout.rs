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

/// 布局切换按钮文案：显示**将要进入**的模式（对话中显示「编辑器」，编辑器中显示「对话」）。
pub fn ide_layout_toggle_label(l: Locale, editor_layout_mode: bool) -> &'static str {
    if editor_layout_mode {
        ide_toggle_chat(l)
    } else {
        ide_toggle_editor(l)
    }
}

/// 布局切换按钮 `aria-label`（与 [`ide_layout_toggle_label`] 对应）。
pub fn ide_layout_toggle_aria(l: Locale, editor_layout_mode: bool) -> &'static str {
    if editor_layout_mode {
        ide_toggle_chat_aria(l)
    } else {
        ide_toggle_editor_aria(l)
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
        Locale::ZhHans => "单击或双击文件在新标签页打开。",
        Locale::En => "Click or double-click a file to open it in a new tab.",
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

pub fn ide_tab_ctx_close(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭",
        Locale::En => "Close",
    }
}

pub fn ide_tab_ctx_close_others(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭其他",
        Locale::En => "Close Others",
    }
}

pub fn ide_tab_ctx_close_all(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "全部关闭",
        Locale::En => "Close All",
    }
}

pub fn ide_tab_ctx_pin(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "固定",
        Locale::En => "Pin",
    }
}

pub fn ide_tab_ctx_unpin(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "取消固定",
        Locale::En => "Unpin",
    }
}

pub fn ide_tab_pinned_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已固定",
        Locale::En => "Pinned",
    }
}
