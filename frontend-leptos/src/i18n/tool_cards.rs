use super::Locale;

// --- message_format / 工具卡 ---

pub fn tool_card_prefix(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具：",
        Locale::En => "Tool: ",
    }
}

pub fn tool_card_fallback(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "工具输出",
        Locale::En => "Tool output",
    }
}

pub fn plan_generated(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "已生成分阶段规划。",
        Locale::En => "Staged plan generated.",
    }
}

pub fn plan_step_no_desc(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "(未提供描述)",
        Locale::En => "(no description)",
    }
}

pub fn plan_step_placeholder_id() -> &'static str {
    "step"
}

pub fn plan_step_line(l: Locale, idx: usize, id: &str, desc: &str) -> String {
    let n = idx + 1;
    match l {
        Locale::ZhHans => format!("{n}. `{id}`: {desc}"),
        Locale::En => format!("{n}. `{id}`: {desc}"),
    }
}
