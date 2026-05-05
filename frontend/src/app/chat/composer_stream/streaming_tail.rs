//! 流式助手尾气泡：当前占位消息 id、工具后钉尾标记、无 `tool_call_id` 时的工具卡片 FIFO。
//!
//! 与 [`super::context::ChatStreamCallbackCtx`] 其他字段解耦，便于统一约定「谁在何时改写 `assistant_message_id`」。

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

/// 单次 `/chat/stream` attach 内共享的尾气泡可变状态（仍在同一任务队列上变异，无需 `Sync`）。
pub(super) struct StreamingAssistantTail {
    assistant_message_id: RefCell<String>,
    post_tool_stream_tail: Cell<bool>,
    pending_tool_message_ids: Rc<RefCell<VecDeque<String>>>,
}

impl StreamingAssistantTail {
    pub(super) fn new(initial_asst_id: String) -> Self {
        Self {
            assistant_message_id: RefCell::new(initial_asst_id),
            post_tool_stream_tail: Cell::new(false),
            pending_tool_message_ids: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    #[inline]
    pub(super) fn borrow_assistant_id(&self) -> std::cell::Ref<'_, String> {
        self.assistant_message_id.borrow()
    }

    #[inline]
    pub(super) fn clone_assistant_id(&self) -> String {
        self.assistant_message_id.borrow().clone()
    }

    #[inline]
    pub(super) fn replace_assistant_id(&self, id: String) {
        self.assistant_message_id.replace(id);
    }

    #[inline]
    pub(super) fn post_tool_stream_tail_cell(&self) -> &Cell<bool> {
        &self.post_tool_stream_tail
    }

    /// 与 `on_tool_result` / `on_tool_call` 共用队列句柄。
    #[inline]
    pub(super) fn pending_tool_message_ids(&self) -> Rc<RefCell<VecDeque<String>>> {
        Rc::clone(&self.pending_tool_message_ids)
    }
}
