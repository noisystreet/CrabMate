//! 单次 `attach` 内 SSE 回调共享的**纯本地**累计状态（与 [`super::context::ChatStreamCallbackCtx`] 分层：
//! `ChatStreamCallbackCtx` 绑定会话与 shell；本类型只承载「这一轮流的计数与标记」，
//! 避免在 [`super::callbacks::assemble`] 与 `builders` 之间传递四个独立 `Rc<Cell/RefCell>`。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// 一轮 `/chat/stream` 生命周期内共享的可变累计（非 `Sync`，仅在同一线程任务队列上使用）。
pub(super) struct PerStreamAccum {
    pub(super) answer_delta_chars: Cell<usize>,
    pub(super) stream_end_reason: RefCell<Option<String>>,
    pub(super) current_subgoal_marker: RefCell<Option<String>>,
    pub(super) saw_final_response_timeline: Cell<bool>,
}

impl PerStreamAccum {
    #[must_use]
    pub(super) fn new_rc() -> Rc<Self> {
        Rc::new(Self {
            answer_delta_chars: Cell::new(0),
            stream_end_reason: RefCell::new(None),
            current_subgoal_marker: RefCell::new(None),
            saw_final_response_timeline: Cell::new(false),
        })
    }
}
