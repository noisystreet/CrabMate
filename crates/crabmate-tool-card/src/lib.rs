//! 工具结果卡 **compact / detail** 的共享生成（Web SSE、水合、`GET /conversation/messages` 快照）。

mod card;
mod input;
#[allow(dead_code)]
mod locale;
mod parse;
mod plain;
mod stored;
mod strip_ansi;

pub use card::{tool_card_compact_text, tool_card_text};
pub use input::{NormalizedToolSnapshotFields, ToolCardInput};
pub use locale::ToolCardLocale;
pub use parse::{looks_like_crabmate_tool_envelope, parse_tool_envelope};
pub use stored::{ToolStoredText, tool_stored_text, tool_stored_text_from_envelope};
