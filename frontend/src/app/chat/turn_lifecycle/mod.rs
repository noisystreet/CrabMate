//! 单轮 `/chat/stream` 的**粗粒度生命周期**（UI 门闩与 attach 代际的唯一来源）。
//!
//! 与 [`super::composer_stream::stream_control_reducer`] 分工：后者描述 attach 内 SSE 消费进度；
//! 本模块收敛壳层 busy 语义，供 [`crate::chat_session_state::make_chat_stream_busy_memos`] 读取。

mod reducer;

#[cfg(test)]
mod tests;

pub(crate) use reducer::{
    TurnLifecycleEvent, TurnLifecycleState, apply_turn_lifecycle, turn_lifecycle_model_ui_busy,
    turn_lifecycle_stream_turn_busy, turn_lifecycle_tool_ui_busy,
};
