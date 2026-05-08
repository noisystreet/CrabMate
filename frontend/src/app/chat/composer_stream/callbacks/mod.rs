//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口。
//!
//! - [`helpers`]：文本拼接、时间线插入、子目标合并等辅助逻辑。
//! - [`done_session`]：`on_done` 对会话 `messages` 的尾泡写回。
//! - [`delta_apply`]：`on_delta` 车道轮换与正文/思维链写入。
//! - [`builders`]：`on_tool_result` / `on_timeline_log` / `on_done` / `on_error` / `on_ws` / `on_tool_call` 等闭包工厂。
//! - [`done_bubble`]：`on_done` 尾泡收尾的纯函数决策与单测。
//! - [`assemble`]：装配完整的 [`crate::api::ChatStreamCallbacks`]。
//! - [`stream_turn_state`]：模型输出车道（reasoning / 正文 / 待轮换）显式枚举，替代交叉读写的 `Cell<bool>` 对。

mod assemble;
mod builders;
mod delta_apply;
mod done_bubble;
mod done_session;
mod helpers;
mod stream_session_access;
pub(super) mod stream_turn_state;

pub(super) use assemble::build_chat_stream_callbacks;

#[cfg(test)]
mod tests;
