//! 工具结果卡片：薄封装，正文在共享 crate [`crabmate_tool_card`]。

use crabmate_tool_card::{
    ToolCardInput, ToolCardLocale, tool_card_compact_text as shared_compact,
    tool_card_text as shared_detail,
};

use crate::i18n::Locale;
use crate::sse_dispatch::ToolResultInfo;

fn tool_card_locale(loc: Locale) -> ToolCardLocale {
    match loc {
        Locale::ZhHans => ToolCardLocale::ZhHans,
        Locale::En => ToolCardLocale::En,
    }
}

#[must_use]
pub fn tool_result_to_card_input(info: &ToolResultInfo) -> ToolCardInput {
    ToolCardInput {
        name: info.name.clone(),
        goal_id: info.goal_id.clone(),
        tool_call_id: info.tool_call_id.clone(),
        result_version: info.result_version,
        summary: info.summary.clone(),
        output: info.output.clone(),
        ok: info.ok,
        exit_code: info.exit_code,
        error_code: info.error_code.clone(),
        failure_category: info.failure_category.clone(),
        structured_preview: info.structured_preview.clone(),
    }
}

#[must_use]
#[allow(dead_code)] // 对外 API；主路径经 `stored_message::tool_stored_text_from_result_info`。
pub fn tool_card_text(info: &ToolResultInfo, loc: Locale) -> String {
    shared_detail(&tool_result_to_card_input(info), tool_card_locale(loc))
}

#[must_use]
#[allow(dead_code)]
pub fn tool_card_compact_text(info: &ToolResultInfo, loc: Locale) -> String {
    shared_compact(&tool_result_to_card_input(info), tool_card_locale(loc))
}
