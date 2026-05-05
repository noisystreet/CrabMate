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
