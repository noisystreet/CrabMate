//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口。
//!
//! - [`helpers`]：文本拼接、时间线插入、子目标合并等辅助逻辑。
//! - [`builders`]：`on_tool_result` / `on_timeline_log` / `on_delta` / `on_done` / `on_error` / `on_ws` / `on_tool_call` 等闭包工厂。
//! - [`assemble`]：装配完整的 [`crate::api::ChatStreamCallbacks`]。

mod assemble;
mod builders;
mod helpers;
mod stream_session_access;

pub(super) use assemble::build_chat_stream_callbacks;

#[cfg(test)]
mod tests;
