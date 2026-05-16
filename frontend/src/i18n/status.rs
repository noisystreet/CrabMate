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

pub fn status_chip_context(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "上下文",
        Locale::En => "Context",
    }
}

/// 状态栏「上下文」芯片 `title`：说明 tiktoken 粗估与上限含义。
pub fn status_chip_context_tooltip(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => {
            "当前会话消息体的 prompt tokens（tiktoken 粗估，与出站消息一致）相对 llm_context_tokens 上限；不含工具 JSON，与网关真实计费可能有偏差。"
        }
        Locale::En => {
            "Prompt tokens for stored message bodies (tiktoken estimate, aligned with outbound messages) vs llm_context_tokens cap; excludes tool JSON and may differ from provider billing."
        }
    }
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

/// 用户点击「停止」后，工具占位气泡上替代「执行中」的短标签。
pub fn status_tool_stopped_user(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已终止",
        Locale::En => "Stopped",
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
