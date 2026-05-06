//! 全屏终端 UI（**实验性**）。阶段 A/B：终端恢复 + Web 对齐式分区；**阶段 C**：与 REPL 共用 `repl_dispatch_chat_round`，**`/api-key`** 同步接入；stdout 助手渲染关闭（见 **`run_session`**）。
//!
//! 入口：**`crabmate tui`**（须交互式 TTY）。

mod llm_stream_scratch;
mod run_session;

pub use llm_stream_scratch::{TuiLlmStreamScratch, TuiLlmStreamScratchArc};
pub use run_session::run_tui_session;
