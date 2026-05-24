//! 工具结果写入 [`crate::storage::StoredMessage`] 的**单一入口**（SSE 与水合共用）。

pub use crabmate_tool_card::{ToolStoredText, tool_stored_text};

use crate::i18n::Locale;
use crate::sse_dispatch::ToolResultInfo;

use super::tool_card::tool_result_to_card_input;

/// 由 SSE [`ToolResultInfo`] 生成与持久化一致的展示字段。
#[must_use]
pub fn tool_stored_text_from_result_info(info: &ToolResultInfo, loc: Locale) -> ToolStoredText {
    let card_loc = match loc {
        Locale::ZhHans => crabmate_tool_card::ToolCardLocale::ZhHans,
        Locale::En => crabmate_tool_card::ToolCardLocale::En,
    };
    tool_stored_text(&tool_result_to_card_input(info), card_loc)
}
