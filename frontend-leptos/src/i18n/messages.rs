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

/// 分阶段规划 SSE `executor_kind` 蛇形值 → 短标签（时间线用）。
pub fn staged_executor_kind_short_label(l: Locale, kind: &str) -> String {
    match kind.trim() {
        "review_readonly" => match l {
            Locale::ZhHans => "只读审阅".to_string(),
            Locale::En => "read-only".to_string(),
        },
        "patch_write" => match l {
            Locale::ZhHans => "补丁修改".to_string(),
            Locale::En => "patch".to_string(),
        },
        "test_runner" => match l {
            Locale::ZhHans => "测试".to_string(),
            Locale::En => "tests".to_string(),
        },
        t if !t.is_empty() => match l {
            Locale::ZhHans => format!("角色 {t}"),
            Locale::En => format!("role {t}"),
        },
        _ => String::new(),
    }
}

fn staged_step_status_line(l: Locale, status: &str) -> String {
    match status {
        "ok" => match l {
            Locale::ZhHans => "完成".to_string(),
            Locale::En => "done".to_string(),
        },
        "failed" => match l {
            Locale::ZhHans => "失败".to_string(),
            Locale::En => "failed".to_string(),
        },
        "cancelled" => match l {
            Locale::ZhHans => "已取消".to_string(),
            Locale::En => "cancelled".to_string(),
        },
        _ => status.to_string(),
    }
}

/// 时间线旁注：单步开始（**不**进入模型上下文；**无**「分阶段 ·」前缀，以 **`{step_index}.`** 编号开头）。
pub fn timeline_staged_step_started(
    l: Locale,
    step_index: usize,
    _total_steps: usize,
    description: &str,
    executor_kind: Option<&str>,
) -> String {
    const MAX_DESC: usize = 72;
    let mut d = description.trim().to_string();
    if d.chars().count() > MAX_DESC {
        d = d.chars().take(MAX_DESC).collect::<String>();
        d.push('…');
    }
    let role = executor_kind
        .map(|k| staged_executor_kind_short_label(l, k))
        .filter(|s| !s.is_empty());
    let role_sep = match l {
        Locale::ZhHans => " · ",
        Locale::En => " · ",
    };
    let ord = step_index.max(1);
    let core = match (&role, d.is_empty()) {
        (Some(r), false) => format!("{r}{role_sep}{d}"),
        (Some(r), true) => r.to_string(),
        (None, false) => d,
        (None, true) => String::new(),
    };
    if core.is_empty() {
        format!("{ord}.")
    } else {
        format!("{ord}. {core}")
    }
}

/// 时间线旁注：单步结束（**无**「分阶段 ·」前缀，以 **`{step_index}.`** 编号开头）。
pub fn timeline_staged_step_finished(
    l: Locale,
    step_index: usize,
    _total_steps: usize,
    status: &str,
    executor_kind: Option<&str>,
) -> String {
    let st = staged_step_status_line(l, status);
    let role = executor_kind
        .map(|k| staged_executor_kind_short_label(l, k))
        .filter(|s| !s.is_empty());
    let tail = match &role {
        Some(r) => match l {
            Locale::ZhHans => format!(" · {r}"),
            Locale::En => format!(" · {r}"),
        },
        None => String::new(),
    };
    let ord = step_index.max(1);
    format!("{ord}. {st}{tail}")
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

/// 聚合待办卡片标题（**无**「分阶段」字样）。
pub fn staged_plan_todo_title(l: Locale) -> String {
    match l {
        Locale::ZhHans => "规划步骤".to_string(),
        Locale::En => "Plan steps".to_string(),
    }
}

pub fn staged_plan_todo_region_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "规划步骤列表",
        Locale::En => "Plan step list",
    }
}

pub fn staged_plan_todo_step_done_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已完成",
        Locale::En => "Completed",
    }
}

pub fn staged_plan_todo_step_in_progress_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "进行中",
        Locale::En => "In progress",
    }
}

pub fn staged_plan_todo_step_pending_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "待执行",
        Locale::En => "Pending",
    }
}

pub fn staged_plan_todo_step_failed_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "失败",
        Locale::En => "Failed",
    }
}

pub fn staged_plan_todo_step_cancelled_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已取消",
        Locale::En => "Cancelled",
    }
}

pub fn staged_plan_todo_legacy_note(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "历史旁注",
        Locale::En => "Legacy notes",
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
