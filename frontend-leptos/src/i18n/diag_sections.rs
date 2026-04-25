//! 错误/工具卡多段说明共用标题（「发生了什么」等），与 `composer_stream` / `tool_card` 对齐。

use super::Locale;

/// 流式错误、`tool_card` 失败块首段标题。
pub fn diag_error_what_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "发生了什么",
        Locale::En => "What happened",
    }
}

pub fn diag_error_impact_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "影响范围",
        Locale::En => "Impact",
    }
}

pub fn diag_error_next_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "建议下一步",
        Locale::En => "Next step",
    }
}

/// 工具成功展开块三段标题。
pub fn diag_success_done_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "完成了什么",
        Locale::En => "What was done",
    }
}

pub fn diag_success_produced_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "产出是什么",
        Locale::En => "What was produced",
    }
}

pub fn diag_success_next_title(l: Locale) -> &'static str {
    match l {
        Locale::ZhHans => "可继续做什么",
        Locale::En => "What next",
    }
}

/// 流式/工具失败说明三段（与 `build_stream_error_with_suggestion` 同形）。
pub fn format_error_three_part(
    l: Locale,
    happened: &str,
    impact: &str,
    suggestion: &str,
) -> String {
    format!(
        "{}\n{happened}\n\n{}\n{impact}\n\n{}\n{suggestion}",
        diag_error_what_title(l),
        diag_error_impact_title(l),
        diag_error_next_title(l),
    )
}

/// 工具成功展开三段（与 `build_tool_success_block` 同形）。
pub fn format_success_three_part(l: Locale, done: &str, output: &str, next: &str) -> String {
    format!(
        "{}\n{done}\n\n{}\n{output}\n\n{}\n{next}",
        diag_success_done_title(l),
        diag_success_produced_title(l),
        diag_success_next_title(l),
    )
}
