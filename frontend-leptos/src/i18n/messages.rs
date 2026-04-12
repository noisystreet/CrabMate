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

/// 时间线旁注：分阶段单步开始（**不**进入模型上下文）。
pub fn timeline_staged_step_started(
    l: Locale,
    step_index: usize,
    total_steps: usize,
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
    match l {
        Locale::ZhHans => {
            let mid = match &role {
                Some(r) => format!("{role_sep}{r}"),
                None => String::new(),
            };
            if d.is_empty() {
                format!("分阶段 · 第 {step_index}/{total_steps} 步{mid}")
            } else {
                format!("分阶段 · 第 {step_index}/{total_steps} 步{mid} · {d}")
            }
        }
        Locale::En => {
            let mid = match &role {
                Some(r) => format!("{role_sep}{r}"),
                None => String::new(),
            };
            if d.is_empty() {
                format!("Staged · step {step_index}/{total_steps}{mid}")
            } else {
                format!("Staged · step {step_index}/{total_steps}{mid} · {d}")
            }
        }
    }
}

/// 时间线旁注：分阶段单步结束。
pub fn timeline_staged_step_finished(
    l: Locale,
    step_index: usize,
    total_steps: usize,
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
    match l {
        Locale::ZhHans => format!("分阶段 · 第 {step_index}/{total_steps} 步 {st}{tail}"),
        Locale::En => format!("Staged · step {step_index}/{total_steps} {st}{tail}"),
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

pub fn msg_staged_timeline_run_group_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "连续分阶段实施记录",
        Locale::En => "Consecutive staged plan step records",
    }
}

pub fn msg_staged_timeline_run_count(l: Locale, n: usize) -> String {
    match l {
        Locale::ZhHans => format!("{n} 条分阶段记录"),
        Locale::En => {
            if n == 1 {
                "1 staged step record".to_string()
            } else {
                format!("{n} staged step records")
            }
        }
    }
}

pub fn msg_staged_timeline_collapse_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "折叠分阶段记录",
        Locale::En => "Collapse staged records",
    }
}

pub fn msg_staged_timeline_collapse_aria(l: Locale) -> &'static str {
    msg_staged_timeline_collapse_title(l)
}

pub fn msg_staged_timeline_collapse_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "折叠",
        Locale::En => "Collapse",
    }
}

pub fn msg_staged_timeline_expand_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开分阶段记录",
        Locale::En => "Expand staged records",
    }
}

pub fn msg_staged_timeline_expand_aria(l: Locale) -> &'static str {
    msg_staged_timeline_expand_title(l)
}

pub fn msg_staged_timeline_expand_btn(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "展开",
        Locale::En => "Expand",
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
