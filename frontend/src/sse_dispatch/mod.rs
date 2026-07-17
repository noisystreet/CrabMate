//! 前端 SSE / AG-UI 控制面类型定义（载荷形状与回调分组类型）。
//!
//! 当前仅 AG-UI（v2）协议；`V2Parser` 将事件分发到 `SseControlSink` 回调。

mod types;

pub use types::*;
