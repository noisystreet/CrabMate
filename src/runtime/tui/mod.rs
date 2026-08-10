//! 全屏终端 UI（**实验性**）。阶段 A/B：终端恢复 + Web 对齐式分区；**阶段 C**：与 REPL 共用 `repl_dispatch_chat_round`，**`/api-key`** 同步接入；stdout 助手渲染关闭（见 **`run_session`**）。
//!
//! **`crabmate tui` 入口已移除**（D2.1）；本模块暂留至 D2.2 删除。
#![allow(dead_code)]

mod run_session;

pub use crabmate_llm::stream_scratch::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
#[allow(unused_imports)] // D2.1：入口已移除；D2.2 删模块
pub use run_session::run_tui_session;
