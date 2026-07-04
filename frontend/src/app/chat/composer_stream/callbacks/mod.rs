//! 将 SSE 各事件装配为 [`ChatStreamCallbacks`]：与 `send_chat_stream` 对齐的单一出口。
//!
//! - [`helpers`]：文本拼接、时间线插入、子目标合并等辅助逻辑。
//! - [`done_session`]：`on_done` 对会话 `messages` 的尾泡写回。
//! - [`error_session`]：`on_error` 对会话 `messages` 的尾助手泡写回。
//! - [`delta_apply`]：`on_delta` 车道轮换与正文/思维链写入。
//! - [`builders/`]：`on_tool_result` / `on_timeline_log` / `on_done` / `on_error` / `on_ws` / `on_tool_call` 等闭包工厂。
//! - [`done_bubble`]：`on_done` 尾泡收尾的纯函数决策与单测。
//! - [`assemble`]：装配完整的 [`crate::api::ChatStreamCallbacks`]。
//! - [`super::stream_turn_scratch_state`]：单轮流 lane / 尾泡 / 工具 FIFO 的状态方法（语义见 **`StreamModelOutputLane`**）。
//! - [`super::stream_turn_state`]：模型输出车道枚举与底层 `lane_*`（供 `stream_turn_scratch_state` 使用）。
//! - [`turn_layout`]：单轮 `messages` 布局状态机（工具 / 时间线 / 尾泡顺序）。
//! - [`stream_session_access`]：**`append_stream_assistant_chunk`** / **`ChatStreamCallbackCtx::append_assistant_chunk`**、绑定会话读写（**`with_stream_write_session_*`** 与 **`ChatStreamCallbackCtx::update_bound_session` / `read_bound_session`**）。

mod assemble;
mod builders;
mod delta_apply;
mod done_bubble;
mod done_session;
mod error_session;
mod helpers;
mod stream_session_access;
mod turn_layout;

pub(super) use assemble::build_chat_stream_callbacks;
pub(crate) use turn_layout::BubbleOutputQueue;
pub(super) use turn_layout::TurnLayout;

#[cfg(test)]
mod tests;
