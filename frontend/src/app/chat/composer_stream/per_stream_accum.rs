//! 单次 `attach` 内 SSE 回调共享的**纯本地**累计状态（与 [`super::context::ChatStreamCallbackCtx`] 分层：
//! `ChatStreamCallbackCtx` 绑定会话与 shell；本类型只承载「这一轮流的计数与标记」，
//! 避免在 [`super::callbacks::assemble`] 与 `builders` 之间传递四个独立 `Rc<Cell/RefCell>`。
//!
//! ## 维护约定
//! - **增量写入**：请优先使用 [`PerStreamAccum`] 上的方法（如 [`Self::add_answer_delta_chars`]），
//!   避免在 `assemble` / `builders` / `helpers` 中散落 `.get()` / `.borrow_mut()`。
//! - **回合收尾**：[`Self::summarize_for_stream_done`] 一次性拷贝当前四项状态，供 `on_done` 决策；
//!   以后若新增累计字段，须同步：`new_rc` 初值、对应 setter/累计方法、以及 [`PerStreamTurnSummary`]。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// `on_done` / 诊断用的「本轮流」快照（从 [`PerStreamAccum`] 拷贝，避免多处 borrow）。
#[derive(Clone)]
pub(super) struct PerStreamTurnSummary {
    pub(super) answer_delta_chars: usize,
    pub(super) stream_end_reason: Option<String>,
    pub(super) saw_final_response_timeline: bool,
    /// 与回合收尾决策无关时会闲置；仍纳入快照以免日后扩展 `on_done` 时再散落 borrow。
    #[allow(dead_code)]
    pub(super) current_subgoal_marker: Option<String>,
}

/// 一轮 `/chat/stream` 生命周期内共享的可变累计（非 `Sync`，仅在同一线程任务队列上使用）。
pub(super) struct PerStreamAccum {
    answer_delta_chars: Cell<usize>,
    stream_end_reason: RefCell<Option<String>>,
    current_subgoal_marker: RefCell<Option<String>>,
    saw_final_response_timeline: Cell<bool>,
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

    /// 回合结束（`on_done`）前调用：一次性读取全部累计，避免闭包内遗漏字段。
    pub(super) fn summarize_for_stream_done(&self) -> PerStreamTurnSummary {
        PerStreamTurnSummary {
            answer_delta_chars: self.answer_delta_chars.get(),
            stream_end_reason: self.stream_end_reason.borrow().clone(),
            saw_final_response_timeline: self.saw_final_response_timeline.get(),
            current_subgoal_marker: self.current_subgoal_marker.borrow().clone(),
        }
    }

    pub(super) fn set_stream_end_reason(&self, reason: String) {
        *self.stream_end_reason.borrow_mut() = Some(reason);
    }

    pub(super) fn clear_answer_delta_chars(&self) {
        self.answer_delta_chars.set(0);
    }

    pub(super) fn add_answer_delta_chars(&self, n: usize) {
        self.answer_delta_chars
            .set(self.answer_delta_chars.get().saturating_add(n));
    }

    pub(super) fn set_saw_final_response_timeline(&self, v: bool) {
        self.saw_final_response_timeline.set(v);
    }

    pub(super) fn set_current_subgoal_marker(&self, marker: Option<String>) {
        *self.current_subgoal_marker.borrow_mut() = marker;
    }

    pub(super) fn current_subgoal_marker_cloned(&self) -> Option<String> {
        self.current_subgoal_marker.borrow().clone()
    }
}
