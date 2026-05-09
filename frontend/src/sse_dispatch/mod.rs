//! 前端 SSE 控制面 JSON 的分类与分发（`serde_json::Value`）。
//!
//! **`stop`/`handled`/`plain` 分支顺序**须与 workspace crate **`crabmate-sse-protocol`** 中
//! [`classify_sse_control_outcome`](crabmate_sse_protocol::classify_sse_control_outcome) 及
//! **`fixtures/sse_control_golden.jsonl`** 一致（见该 crate 的 `control_classify`）。
//!
//! 实现拆分：**`types`**（载荷与 sink 类型）、**`dispatch`**（`try_dispatch` 与各分支）、本文件再导出以保持 **`crate::sse_dispatch::…`** 路径不变。

mod dispatch;
#[cfg(test)]
mod sse_control_order_tests;
mod types;

pub use dispatch::try_dispatch_sse_control_payload;
pub use types::*;
