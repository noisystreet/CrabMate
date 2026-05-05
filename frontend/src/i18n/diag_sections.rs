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
