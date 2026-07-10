//! 单轮 `/chat/stream` 的**粗粒度生命周期**（阶段 B：观测 + 单测；UI 仍用 legacy busy `Memo`）。
//!
//! 与 [`super::composer_stream::stream_control_reducer`] 分工：后者描述 attach 内 SSE 消费进度；
//! 本模块收敛壳层 `StreamRunPhase` / busy 语义，供后续替代 [`crate::chat_session_state::make_chat_stream_busy_memos`] 的 OR 门闩。

mod reducer;

#[cfg(test)]
mod tests;

pub(crate) use reducer::{
    TurnLifecycleEvent, TurnLifecycleState, apply_turn_lifecycle, turn_lifecycle_coarse_busy,
    turn_lifecycle_ui_inflight,
};
