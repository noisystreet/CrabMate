use super::Locale;

// --- 消息气泡 ---

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

pub fn msg_tool_run_group_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "连续工具输出",
        Locale::En => "Consecutive tool output",
    }
}

pub fn msg_tool_run_count(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 条工具输出"),
        Locale::En => {
            if n == 1 {
                "1 tool output".to_string()
            } else {
                format!("{n} tool outputs")
            }
        }
    }
}

pub fn msg_tool_collapse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "折叠连续工具输出",
        Locale::En => "Collapse tool outputs",
    }
}

pub fn msg_tool_collapse_aria(l: Locale) -> &'static str {
    msg_tool_collapse_title(l)
}

pub fn msg_tool_collapse_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "折叠",
        Locale::En => "Collapse",
    }
}

pub fn msg_tool_expand_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开连续工具输出",
        Locale::En => "Expand tool outputs",
    }
}

pub fn msg_tool_expand_aria(l: Locale) -> &'static str {
    msg_tool_expand_title(l)
}

pub fn msg_tool_expand_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开",
        Locale::En => "Expand",
    }
}

pub fn msg_tool_detail_expand_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开工具输出详情",
        Locale::En => "Expand tool output details",
    }
}

pub fn msg_tool_detail_collapse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "收起工具输出详情",
        Locale::En => "Collapse tool output details",
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

/// 与分层子目标顶栏同构：单条分步时间线气泡的横幅文案。
#[expect(dead_code)]
pub fn msg_staged_timeline_exec_banner(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分步执行",
        Locale::En => "Plan step",
    }
}

pub fn msg_jump_user_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "点击跳转到对应用户消息",
        Locale::En => "Jump to related user message",
    }
}

pub fn msg_jump_user_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "跳转到对应用户消息",
        Locale::En => "Jump to user message",
    }
}

pub fn msg_planner_round_badge(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "规划轮",
        Locale::En => "Planner",
    }
}

pub fn msg_planner_round_badge_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "本条为分阶段规划模型输出（含结构化 agent_reply_plan）",
        Locale::En => "Staged planning turn (structured agent_reply_plan)",
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

pub fn msg_copy_aria(l: Locale) -> &'static str {
    msg_copy_title(l)
}

pub fn msg_regen_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除本条及之后消息并重新生成（服务端会话需已持久化）",
        Locale::En => "Delete from here and regenerate (server session must be persisted)",
    }
}

pub fn msg_regen_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "从此处重试",
        Locale::En => "Regenerate from here",
    }
}

pub fn msg_branch_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "删除本条及之后消息（不自动发送；服务端会话同步截断需已持久化）",
        Locale::En => "Branch: delete from here (no auto-send; server sync needs persistence)",
    }
}

pub fn msg_branch_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "分支对话",
        Locale::En => "Branch conversation",
    }
}

pub fn msg_retry_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试当前助手生成",
        Locale::En => "Retry assistant generation",
    }
}

pub fn msg_retry_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试",
        Locale::En => "Retry",
    }
}
