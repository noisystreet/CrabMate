use super::Locale;

// --- 状态栏 ---

pub fn status_fetch_error(l: Locale, err: &str) -> String {
    match l {
        Locale::ZhHans => format!("无法加载状态（/status）：{err}"),
        Locale::En => format!("Failed to load status (/status): {err}"),
    }
}

pub fn status_retry(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "重试",
        Locale::En => "Retry",
    }
}

pub fn status_loading_aria(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "加载状态",
        Locale::En => "Loading status",
    }
}

pub fn status_chip_model(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型",
        Locale::En => "Model",
    }
}

pub fn status_chip_base_url(_l: Locale) -> &'static str {
    "base_url"
}

pub fn status_role_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "角色",
        Locale::En => "Role",
    }
}

pub fn status_role_title_attr(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "Agent 角色（对标 CLI /agent set）",
        Locale::En => "Agent role (same as CLI /agent set)",
    }
}

pub fn status_context_label(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文",
        Locale::En => "Context",
    }
}

pub fn status_context_title(l: Locale, used: usize, budget: usize, over_budget: bool) -> String {
    match l {
        Locale::ZhHans => {
            let hint = if over_budget {
                "已超过配置的字符预算，服务端可能在请求前裁剪历史。"
            } else {
                "与服务端注入/裁剪策略大致对照，非 token 精确值。"
            };
            format!(
                "本地估算：当前会话消息 + 输入草稿约 {used} 字符；context_char_budget={budget}。{hint}"
            )
        }
        Locale::En => {
            let hint = if over_budget {
                "Over the configured char budget; the server may trim older turns before the request."
            } else {
                "Rough local estimate vs server char budget—not exact tokens."
            };
            format!(
                "Local estimate: messages + composer draft ≈ {used} chars; context_char_budget={budget}. {hint}"
            )
        }
    }
}

pub fn status_context_title_no_budget(l: Locale, used: usize) -> String {
    match l {
        Locale::ZhHans => {
            format!("本地估算约 {used} 字符（context_char_budget=0，未启用按字符预算对照）")
        }
        Locale::En => format!(
            "Local estimate ≈ {used} chars (context_char_budget=0; no char budget configured)"
        ),
    }
}

pub fn status_context_meta_chars(l: Locale, used: usize) -> String {
    match l {
        Locale::ZhHans => format!("{used} 字"),
        Locale::En => format!("{used} ch"),
    }
}

pub fn status_context_meta_pct(l: Locale, pct: u32) -> String {
    match l {
        Locale::ZhHans => format!("{pct}%"),
        Locale::En => format!("{pct}%"),
    }
}

pub fn status_context_rev(l: Locale, rev: u64) -> String {
    match l {
        Locale::ZhHans => format!("rev {rev}"),
        Locale::En => format!("rev {rev}"),
    }
}

pub fn status_default_option(l: Locale, id: Option<&str>) -> String {
    match l {
        Locale::ZhHans => match id {
            Some(i) => format!("default ({i})"),
            None => "default".to_string(),
        },
        Locale::En => match id {
            Some(i) => format!("default ({i})"),
            None => "default".to_string(),
        },
    }
}

pub fn status_unavailable(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "/status 不可用",
        Locale::En => "/status unavailable",
    }
}

pub fn status_error_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "错误: ",
        Locale::En => "Error: ",
    }
}

pub fn status_tool_running(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具执行中…",
        Locale::En => "Running tools…",
    }
}

pub fn status_model_running(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "模型生成中…",
        Locale::En => "Model generating…",
    }
}

pub fn status_ready(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "就绪",
        Locale::En => "Ready",
    }
}
