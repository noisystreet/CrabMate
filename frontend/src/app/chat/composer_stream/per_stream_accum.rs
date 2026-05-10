//! 单次 `attach` 内 SSE 回调共享的**纯本地**累计状态（与 [`super::context::ChatStreamCallbackCtx`] 分层：
//! `ChatStreamCallbackCtx` 绑定会话与 shell；本类型只承载「这一轮流的计数与标记」。
//!
//! 实现上将四项字段收进**单个** [`RefCell`]（[`AccumState`]）：任意读写只触发**一次**借用，
//! 避免多个 `RefCell` 并存带来的交叉借用心智负担，并与「串行 SSE 回调」模型一致。
//!
//! ## 维护约定
//! - **增量写入**：请优先使用 [`PerStreamAccum`] 上的方法（如 [`Self::add_answer_delta_chars`]），
//!   避免在 `assemble` / `builders` / `helpers` 中直接访问 [`AccumState`] 或散落 `.borrow_mut()`。
//! - **回合收尾**：[`Self::summarize_for_stream_done`] 一次性拷贝当前四项状态，供 `on_done` 决策；
//!   以后若新增累计字段，须同步：`AccumState` 初值、对应 setter/累计方法、以及 [`PerStreamTurnSummary`]。

use std::cell::RefCell;
use std::rc::Rc;

/// 一轮流内累积的可变字段（与 [`PerStreamTurnSummary`] 字段一一对应，便于一次性拷贝）。
struct AccumState {
    answer_delta_chars: usize,
    stream_end_reason: Option<String>,
    current_subgoal_marker: Option<String>,
    saw_final_response_timeline: bool,
}

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
    state: RefCell<AccumState>,
}

impl PerStreamAccum {
    #[must_use]
    pub(super) fn new_rc() -> Rc<Self> {
        Rc::new(Self {
            state: RefCell::new(AccumState {
                answer_delta_chars: 0,
                stream_end_reason: None,
                current_subgoal_marker: None,
                saw_final_response_timeline: false,
            }),
        })
    }

    /// 回合结束（`on_done`）前调用：一次性读取全部累计，避免闭包内遗漏字段。
    pub(super) fn summarize_for_stream_done(&self) -> PerStreamTurnSummary {
        let s = self.state.borrow();
        PerStreamTurnSummary {
            answer_delta_chars: s.answer_delta_chars,
            stream_end_reason: s.stream_end_reason.clone(),
            saw_final_response_timeline: s.saw_final_response_timeline,
            current_subgoal_marker: s.current_subgoal_marker.clone(),
        }
    }

    pub(super) fn set_stream_end_reason(&self, reason: String) {
        self.state.borrow_mut().stream_end_reason = Some(reason);
    }

    pub(super) fn clear_answer_delta_chars(&self) {
        self.state.borrow_mut().answer_delta_chars = 0;
    }

    pub(super) fn add_answer_delta_chars(&self, n: usize) {
        let mut s = self.state.borrow_mut();
        s.answer_delta_chars = s.answer_delta_chars.saturating_add(n);
    }

    pub(super) fn set_saw_final_response_timeline(&self, v: bool) {
        self.state.borrow_mut().saw_final_response_timeline = v;
    }

    pub(super) fn set_current_subgoal_marker(&self, marker: Option<String>) {
        self.state.borrow_mut().current_subgoal_marker = marker;
    }

    pub(super) fn current_subgoal_marker_cloned(&self) -> Option<String> {
        self.state.borrow().current_subgoal_marker.clone()
    }
}
