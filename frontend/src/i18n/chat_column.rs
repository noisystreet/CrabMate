use super::Locale;

// --- 聊天列空态 / 输入区 ---

pub fn chat_empty_lead(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "在下方输入消息，Enter 发送，Shift+Enter 换行。",
        Locale::En => "Type below: Enter to send, Shift+Enter for newline.",
    }
}

pub fn chat_empty_tip1(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "左侧可新建对话、切换最近会话；会话项右键或列表空白处右键可「管理会话」、导出与重命名。"
        }
        Locale::En => {
            "Use the left rail for new chat and recent sessions; right-click a session or empty list space for Manage sessions, export, and rename."
        }
    }
}

pub fn chat_empty_tip2(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "侧栏展开时工具栏在右列顶部；「隐藏侧栏」后右侧贴边纵向三键，同宽铺满一条，无额外围框。视图菜单可在隐藏、工作区、任务之间切换。"
        }
        Locale::En => {
            "With the side panel open, tools are on the top of the right column; when hidden, three buttons stack on the right edge. The view menu switches hide / workspace / tasks."
        }
    }
}

pub fn composer_ph(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "输入消息，Enter 发送 / Shift+Enter 换行；@相对路径 可展开文件（与 read_file 一致）…"
        }
        Locale::En => {
            "Message: Enter send / Shift+Enter newline; @rel/path expands file (read_file rules)…"
        }
    }
}

/// 工作区树双击插入 `@路径` 时路径含空白。
pub fn composer_ws_path_whitespace_err(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "该文件路径含空格，无法自动生成 @引用，请手动输入相对路径。",
        Locale::En => {
            "This path contains spaces; cannot auto-insert @ ref — type the relative path manually."
        }
    }
}

/// 侧栏工作区文件行：双击插入到输入框的提示。
pub fn workspace_tree_insert_file_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "双击将 @相对路径 插入到聊天输入框（发送时由服务端展开）",
        Locale::En => {
            "Double-click to insert @relative-path into chat (expanded server-side on send)"
        }
    }
}

pub fn composer_stop(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "停止",
        Locale::En => "Stop",
    }
}

pub fn composer_send_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "发送",
        Locale::En => "Send",
    }
}

pub fn composer_attach_image_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "附加图片",
        Locale::En => "Attach image",
    }
}

pub fn composer_remove_image_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "移除图片",
        Locale::En => "Remove image",
    }
}

pub fn clarification_panel_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "需要补充信息",
        Locale::En => "More information needed",
    }
}

pub fn clarification_submit(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "提交澄清",
        Locale::En => "Submit answers",
    }
}

pub fn clarification_dismiss(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "关闭",
        Locale::En => "Dismiss",
    }
}

pub fn clarification_required_suffix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（必填）",
        Locale::En => " (required)",
    }
}

pub fn clarification_missing_required(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "请填写所有必填项",
        Locale::En => "Please fill all required fields",
    }
}

pub fn clarification_user_bubble_stub(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "（已提交澄清问卷）",
        Locale::En => "(Clarification submitted)",
    }
}
// --- 聊天列空态 ---

pub fn chat_history_load_older(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载更早的消息",
        Locale::En => "Load older messages",
    }
}

pub fn chat_history_loading_older(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "正在加载更早的消息…",
        Locale::En => "Loading older messages…",
    }
}

pub fn chat_empty_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "开始对话",
        Locale::En => "Start a conversation",
    }
}

pub fn debug_console_region_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "思维与工具调试台",
        Locale::En => "Thinking and tool debug console",
    }
}

pub fn debug_console_empty_hint(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "发起流式对话后，推理增量与工具上下文摘要会出现在此（若服务端用 `CM_THINKING_TRACE_ENABLED=0` 关闭则不会有事件）。"
        }
        Locale::En => {
            "After a streamed reply, reasoning deltas and tool context summaries appear here (unless the server disabled traces with `CM_THINKING_TRACE_ENABLED=0`)."
        }
    }
}
