//! 全屏 TUI 下助手流式正文的进程内缓冲（与 Web SSE 并行，**不**经 stdout）。
use std::sync::{Arc, Mutex};

/// 与 [`crate::llm::api::sse_parser`] 内 `reasoning_acc` / `content_acc` 增量对齐的展示缓冲。
#[derive(Default)]
pub struct TuiLlmStreamScratch {
    pub(crate) reasoning: String,
    pub(crate) content: String,
}

impl TuiLlmStreamScratch {
    pub(crate) fn clear(&mut self) {
        self.reasoning.clear();
        self.content.clear();
    }
}

pub type TuiLlmStreamScratchArc = Arc<Mutex<TuiLlmStreamScratch>>;
