use super::Locale;

// --- 消息角色 / 工具与操作文案（TUI 与导出共用）---

pub fn msg_role_user(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "用户",
        Locale::En => "User",
    }
}

pub fn msg_role_assistant(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "助手",
        Locale::En => "Assistant",
    }
}

pub fn msg_role_system(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "系统",
        Locale::En => "System",
    }
}

pub fn msg_role_other(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "其它",
        Locale::En => "Other",
    }
}

pub fn msg_tool_detail_expand_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开工具输出详情",
        Locale::En => "Expand tool output details",
    }
}

/// 服务端注入的 `### 分阶段规划 · …` 首行剥除后正文为空时的短标签（与 [`crate::message_format::display::message_ex`] 序号配合）。
pub fn staged_coach_injection_fallback(l: Locale, ordinal: usize) -> &'static str {
    match l {
        Locale::ZhHans => match ordinal {
            2 => "步骤优化",
            3 => "多规划合并",
            _ => "规划轮",
        },
        Locale::En => match ordinal {
            2 => "Step optimization",
            3 => "Multi-planner merge",
            _ => "Planning round",
        },
    }
}

pub fn msg_actions_group_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "消息操作",
        Locale::En => "Message actions",
    }
}

pub fn msg_copy_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "复制本条展示文本",
        Locale::En => "Copy displayed text",
    }
}

pub fn msg_regen_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除本条及之后消息并重新生成（服务端会话需已持久化）",
        Locale::En => "Delete from here and regenerate (server session must be persisted)",
    }
}

pub fn msg_branch_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除本条及之后消息（不自动发送；服务端会话同步截断需已持久化）",
        Locale::En => "Branch: delete from here (no auto-send; server sync needs persistence)",
    }
}

pub fn msg_retry_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试当前助手生成",
        Locale::En => "Retry assistant generation",
    }
}

pub fn msg_toggle_expand_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开或收起长消息",
        Locale::En => "Expand or collapse long message",
    }
}
